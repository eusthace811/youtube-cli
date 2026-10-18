//! A hand-written MCP server with go-sdk v1.8.0's observable behavior:
//! version negotiation, the initialization gate, error codes and texts, list
//! cache fields, and the multi-round-trip confirmation with its
//! server-issued `elicitation/create` fallback.
//!
//! [`Session::handle`] is transport independent: it takes one decoded
//! message and returns the reply line for a request. Server-initiated
//! requests go out through the session's [`Peer`], and their replies come
//! back in through `handle`. [`serve_stdio`] drives it over newline-delimited
//! JSON on stdin and stdout.

use crate::log::{self, Level, go_quote};
use crate::resources::{self, ReadError};
use crate::tools;
use serde::Deserialize;
use serde_json::value::RawValue;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};
use youtube_core::ops::ServiceProvider;

pub const NAME: &str = "youtube";
pub const INSTRUCTIONS: &str = "Automate YouTube operations";

/// go-sdk's supported versions, newest first.
pub(crate) const PROTOCOL_VERSIONS: &[&str] = &[
    "2026-07-28",
    "2025-11-25",
    "2025-06-18",
    "2025-03-26",
    "2024-11-05",
];
/// The first version whose clients take `inputRequests` instead of
/// server-initiated requests.
pub(crate) const MRTR_VERSION: &str = "2026-07-28";

pub(crate) const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_REQUEST: i64 = -32600;
pub(crate) const INVALID_PARAMS: i64 = -32602;
const UNSUPPORTED_PROTOCOL_VERSION: i64 = -32022;

const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_CLIENT_INFO: &str = "io.modelcontextprotocol/clientInfo";
const META_CLIENT_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";
const META_SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";

/// A JSON-RPC error. Code 0 is what go-sdk sends for plain Go errors.
#[derive(Debug, Clone, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl RpcError {
    fn new(code: i64, message: impl Into<String>) -> RpcError {
        RpcError {
            code,
            message: message.into(),
            data: None,
        }
    }

    fn plain(message: impl Into<String>) -> RpcError {
        RpcError::new(0, message)
    }

    fn invalid_request(detail: &str) -> RpcError {
        RpcError::new(INVALID_REQUEST, format!("invalid request: {detail}"))
    }

    fn to_json(&self) -> Value {
        let mut e = json!({"code": self.code, "message": self.message});
        if let Some(d) = &self.data {
            e["data"] = d.clone();
        }
        e
    }
}

/// One decoded JSON-RPC frame. Params stay raw, because go-sdk quotes the
/// client's exact bytes in some error texts.
#[derive(Debug, Clone)]
pub enum Message {
    Request {
        id: Value,
        method: String,
        params: Option<Box<RawValue>>,
    },
    Notification {
        method: String,
        params: Option<Box<RawValue>>,
    },
    Response {
        id: Value,
        outcome: Result<Value, RpcError>,
    },
}

#[derive(Debug)]
pub enum ParseError {
    /// The text is a prefix of a JSON value; more lines may complete it.
    Incomplete,
    Invalid(String),
}

#[derive(Deserialize)]
struct Wire {
    #[serde(default)]
    jsonrpc: Option<String>,
    #[serde(default)]
    id: Option<Value>,
    #[serde(default)]
    method: Option<String>,
    /// Kept even when `null`, which go-sdk tells apart from absent.
    #[serde(default, deserialize_with = "present")]
    params: Option<Box<RawValue>>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<WireError>,
}

#[derive(Deserialize)]
struct WireError {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    message: String,
    #[serde(default)]
    data: Option<Value>,
}

fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Box<RawValue>>, D::Error> {
    Box::<RawValue>::deserialize(d).map(Some)
}

/// One frame of input: a message, or a JSON-RPC batch of them.
#[derive(Debug, Clone)]
pub enum Incoming {
    Single(Message),
    Batch(Vec<Message>),
}

impl Incoming {
    pub fn parse(text: &str) -> Result<Incoming, ParseError> {
        let invalid = |e: serde_json::Error| {
            if e.is_eof() {
                ParseError::Incomplete
            } else {
                ParseError::Invalid(format!("unmarshaling jsonrpc message: {e}"))
            }
        };
        if !text.trim_start().starts_with('[') {
            let w: Wire = serde_json::from_str(text).map_err(invalid)?;
            return Message::from_wire(w)
                .map(Incoming::Single)
                .map_err(ParseError::Invalid);
        }
        let raw: Vec<Box<RawValue>> = serde_json::from_str(text).map_err(invalid)?;
        if raw.is_empty() {
            return Err(ParseError::Invalid("empty batch".into()));
        }
        let mut msgs = Vec::with_capacity(raw.len());
        for r in raw {
            let w: Wire = serde_json::from_str(r.get())
                .map_err(|e| ParseError::Invalid(format!("unmarshaling jsonrpc message: {e}")))?;
            msgs.push(Message::from_wire(w).map_err(ParseError::Invalid)?);
        }
        Ok(Incoming::Batch(msgs))
    }
}

impl Message {
    fn from_wire(f: Wire) -> Result<Message, String> {
        let tag = f.jsonrpc.unwrap_or_default();
        if tag != "2.0" {
            return Err(format!(
                "invalid message version tag {}; expected \"2.0\"",
                go_quote(&tag)
            ));
        }
        let id = match f.id {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(Value::String(s)),
            Some(Value::Number(n)) => Some(match n.as_f64() {
                Some(x) if n.is_f64() && x.fract() == 0.0 => json!(x as i64),
                _ => Value::Number(n),
            }),
            Some(other) => return Err(format!("invalid ID type {other}")),
        };
        if let Some(method) = f.method {
            return Ok(match id {
                Some(id) => Message::Request {
                    id,
                    method,
                    params: f.params,
                },
                None => Message::Notification {
                    method,
                    params: f.params,
                },
            });
        }
        let id = id.ok_or("invalid request")?;
        let outcome = match f.error {
            Some(e) => Err(RpcError {
                code: e.code,
                message: e.message,
                data: e.data,
            }),
            None => Ok(f.result.unwrap_or(Value::Null)),
        };
        Ok(Message::Response { id, outcome })
    }
}

/// Process-wide server configuration shared by every session.
pub struct Server {
    provider: Arc<dyn ServiceProvider>,
    version: String,
}

impl Server {
    pub fn new(provider: Arc<dyn ServiceProvider>, version: impl Into<String>) -> Arc<Server> {
        Arc::new(Server {
            provider,
            version: version.into(),
        })
    }
}

/// What the client said about itself in `initialize`.
#[derive(Debug, Clone)]
struct Client {
    /// The version the client asked for, which go-sdk (not the negotiated
    /// version) uses to decide whether the client speaks multi round-trip.
    requested_version: String,
    elicitation: Option<Map<String, Value>>,
}

/// The back channel for server-initiated requests.
pub struct Peer {
    out: mpsc::UnboundedSender<String>,
    next_id: AtomicI64,
    pending: Mutex<HashMap<i64, oneshot::Sender<Result<Value, RpcError>>>>,
}

impl Peer {
    pub fn new(out: mpsc::UnboundedSender<String>) -> Peer {
        Peer {
            out,
            next_id: AtomicI64::new(1),
            pending: Mutex::new(HashMap::new()),
        }
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, tx);
        let frame = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if self.out.send(frame.to_string()).is_err() {
            return Err(RpcError::plain("connection closed"));
        }
        rx.await
            .unwrap_or_else(|_| Err(RpcError::plain("connection closed")))
    }

    fn deliver(&self, id: &Value, outcome: Result<Value, RpcError>) {
        let Some(id) = id.as_i64() else { return };
        let waiter = self
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
        if let Some(w) = waiter {
            let _ = w.send(outcome);
        }
    }
}

pub struct Session {
    server: Arc<Server>,
    client: Mutex<Option<Client>>,
    peer: Option<Peer>,
}

/// How a method may be called, from go-sdk's `serverMethodInfos`.
pub(crate) struct MethodInfo {
    pub notification: bool,
    pub params_optional: bool,
}

pub(crate) fn method_info(method: &str) -> Option<MethodInfo> {
    let (notification, params_optional) = match method {
        "initialize"
        | "completion/complete"
        | "prompts/get"
        | "tools/call"
        | "resources/read"
        | "logging/setLevel"
        | "resources/subscribe"
        | "resources/unsubscribe"
        | "subscriptions/listen" => (false, false),
        "server/discover"
        | "ping"
        | "prompts/list"
        | "tools/list"
        | "resources/list"
        | "resources/templates/list" => (false, true),
        "notifications/cancelled"
        | "notifications/initialized"
        | "notifications/roots/list_changed" => (true, true),
        "notifications/progress" => (true, false),
        _ => return None,
    };
    Some(MethodInfo {
        notification,
        params_optional,
    })
}

/// Methods callable before `initialize`, per go-sdk's session gate.
fn allowed_before_init(method: &str) -> bool {
    matches!(
        method,
        "initialize"
            | "ping"
            | "notifications/initialized"
            | "notifications/roots/list_changed"
            | "logging/setLevel"
            | "resources/subscribe"
            | "resources/unsubscribe"
            | "server/discover"
    )
}

type Reply = Result<String, RpcError>;

fn reply(v: Value) -> Reply {
    Ok(v.to_string())
}

impl Session {
    /// `peer` is the back channel for server-initiated requests; without one,
    /// gated tools fail for clients that do not speak multi round-trip.
    pub fn new(server: Arc<Server>, peer: Option<Peer>) -> Arc<Session> {
        Arc::new(Session {
            server,
            client: Mutex::new(None),
            peer,
        })
    }

    /// A stateless HTTP session that skips `initialize`, as go-sdk
    /// synthesizes it: the client asked for `version` and declared no
    /// capabilities, so it has no elicitation back channel.
    pub fn preinitialized(server: Arc<Server>, version: &str) -> Arc<Session> {
        Arc::new(Session {
            server,
            client: Mutex::new(Some(Client {
                requested_version: version.to_string(),
                elicitation: None,
            })),
            peer: None,
        })
    }

    fn client(&self) -> Option<Client> {
        self.client
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn speaks_mrtr(&self) -> bool {
        self.client()
            .is_none_or(|c| c.requested_version.as_str() >= MRTR_VERSION)
    }
    /// Handles one inbound message and returns the reply line for a request.
    pub async fn handle(&self, msg: Message) -> Option<String> {
        match msg {
            Message::Response { id, outcome } => {
                if let Some(p) = &self.peer {
                    p.deliver(&id, outcome);
                }
                None
            }
            // go-sdk's notification handlers only log to a discarded logger.
            Message::Notification { .. } => None,
            Message::Request { id, method, params } => {
                let out = match self.request(&method, params).await {
                    Ok(result) => format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{result}}}"#),
                    Err(mut e) => {
                        // jsonrpc2 recognizes method-not-found by code, whatever
                        // wraps it, and names the incoming method instead.
                        if e.code == METHOD_NOT_FOUND {
                            e.message = format!("method not found: {}", go_quote(&method));
                        }
                        json!({"jsonrpc": "2.0", "id": id, "error": e.to_json()}).to_string()
                    }
                };
                Some(out)
            }
        }
    }

    /// go-sdk's per-request protocol detection (SEP-2575) around the method
    /// dispatch: a call whose `_meta` names 2026-07-28 or later skips the
    /// initialization gate, and its result names the server.
    async fn request(&self, method: &str, raw: Option<Box<RawValue>>) -> Reply {
        let Some(meta) = request_meta(raw.as_deref().map(RawValue::get))? else {
            return self.dispatch(method, raw).await;
        };
        if !PROTOCOL_VERSIONS.contains(&meta.requested_version.as_str()) {
            return Err(RpcError {
                code: UNSUPPORTED_PROTOCOL_VERSION,
                message: "unsupported protocol version".into(),
                data: Some(json!({
                    "supported": PROTOCOL_VERSIONS,
                    "requested": meta.requested_version,
                })),
            });
        }
        match method {
            "initialize"
            | "ping"
            | "logging/setLevel"
            | "resources/subscribe"
            | "resources/unsubscribe" => return Err(RpcError::new(METHOD_NOT_FOUND, "")),
            _ => {}
        }
        {
            let mut client = self.client.lock().unwrap_or_else(PoisonError::into_inner);
            if client.is_none() {
                *client = Some(meta);
            }
        }
        let text = if method == "server/discover" {
            json!({
                "ttlMs": 0,
                "cacheScope": "public",
                "supportedVersions": PROTOCOL_VERSIONS,
                "capabilities": capabilities(),
                "instructions": INSTRUCTIONS,
            })
            .to_string()
        } else {
            self.dispatch(method, raw).await?
        };
        let mut result: Value =
            serde_json::from_str(&text).map_err(|e| RpcError::plain(e.to_string()))?;
        if let Value::Object(m) = &mut result
            && !m.is_empty()
        {
            m.entry("resultType").or_insert(json!("complete"));
            let meta = m.entry("_meta").or_insert_with(|| json!({}));
            if let Value::Object(meta) = meta {
                meta.entry(META_SERVER_INFO)
                    .or_insert_with(|| json!({"name": NAME, "version": self.server.version}));
            }
        }
        reply(result)
    }

    async fn dispatch(&self, method: &str, raw: Option<Box<RawValue>>) -> Reply {
        if !allowed_before_init(method) && self.client().is_none() {
            return Err(RpcError::plain(format!(
                "method {} is invalid during session initialization",
                go_quote(method)
            )));
        }
        if method == "server/discover" {
            return Err(RpcError::new(METHOD_NOT_FOUND, ""));
        }
        let Some(info) = method_info(method) else {
            return Err(RpcError::new(METHOD_NOT_FOUND, ""));
        };
        if info.notification {
            return Err(RpcError::invalid_request(&format!(
                "unexpected id for {}",
                go_quote(method)
            )));
        }
        let raw = raw.as_deref().map(RawValue::get);
        let params = match raw {
            None if !info.params_optional => {
                return Err(RpcError::invalid_request("missing required \"params\""));
            }
            Some("null") if !info.params_optional => {
                let detail = if method == "initialize" {
                    RpcError::plain("missing required \"params\"")
                } else {
                    RpcError::invalid_request("missing required \"params\"")
                };
                return Err(RpcError {
                    message: format!("handling '{method}': {}", detail.message),
                    ..detail
                });
            }
            None | Some("null") => Value::Object(Map::new()),
            Some(text) => match serde_json::from_str(text) {
                Ok(Value::Object(m)) => Value::Object(m),
                _ => {
                    return Err(RpcError::new(
                        INVALID_PARAMS,
                        format!(
                            "handling '{method}': invalid params: unmarshaling {}: json: cannot unmarshal non-object params",
                            go_quote(text)
                        ),
                    ));
                }
            },
        };
        match method {
            "initialize" => self.initialize(&params),
            "ping" | "logging/setLevel" => reply(json!({})),
            "tools/list" => paged(&params, || tools::list_result().to_string()),
            "resources/list" => paged(&params, || resources::list_result().to_string()),
            "resources/templates/list" => {
                paged(&params, || resources::templates_result().to_string())
            }
            "prompts/list" => paged(&params, || {
                json!({"ttlMs": tools::LIST_TTL_MS, "cacheScope": "public", "prompts": []})
                    .to_string()
            }),
            "prompts/get" => Err(RpcError::new(
                INVALID_PARAMS,
                format!("unknown prompt {}", go_quote(str_field(&params, "name"))),
            )),
            "tools/call" => self.call_tool(&params, raw.unwrap_or("{}")).await,
            "resources/read" => self.read_resource(str_field(&params, "uri")).await,
            "completion/complete" => {
                if params.get("ref").is_none_or(Value::is_null) {
                    Err(RpcError::new(
                        INVALID_PARAMS,
                        "invalid params: missing required 'ref' field",
                    ))
                } else if params
                    .pointer("/argument/name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .is_empty()
                {
                    Err(RpcError::new(
                        INVALID_PARAMS,
                        "invalid params: missing required 'argument.name' field",
                    ))
                } else {
                    Err(RpcError::new(METHOD_NOT_FOUND, ""))
                }
            }
            _ => Err(RpcError::new(METHOD_NOT_FOUND, "")),
        }
    }

    fn initialize(&self, params: &Value) -> Reply {
        let requested = str_field(params, "protocolVersion").to_string();
        let elicitation = params
            .pointer("/capabilities/elicitation")
            .and_then(Value::as_object)
            .cloned();
        {
            let mut client = self.client.lock().unwrap_or_else(PoisonError::into_inner);
            if client.is_some() {
                return Err(RpcError::plain("duplicate \"initialize\" received"));
            }
            *client = Some(Client {
                requested_version: requested.clone(),
                elicitation,
            });
        }
        reply(json!({
            "capabilities": capabilities(),
            "instructions": INSTRUCTIONS,
            "protocolVersion": negotiate(&requested),
            "serverInfo": {"name": NAME, "version": self.server.version},
        }))
    }

    /// `raw` is the params text as the client sent it.
    async fn call_tool(&self, params: &Value, raw: &str) -> Reply {
        // go-sdk decodes all params before it looks the tool up.
        let responses = input_responses(params, raw)?;
        let arguments = serde_json::from_str::<CallArguments>(raw)
            .ok()
            .and_then(|c| c.arguments);
        let name = str_field(params, "name");
        let tool = tools::find(name).ok_or_else(|| {
            RpcError::new(INVALID_PARAMS, format!("unknown tool {}", go_quote(name)))
        })?;
        let mrtr = self.speaks_mrtr();
        let mut result = self
            .run_tool(tool, arguments.map(RawValue::get), responses, mrtr)
            .await?;
        if mrtr && result.get("resultType").is_none() {
            result["resultType"] = json!("complete");
        }
        reply(result)
    }

    /// cobra-mcp's `GenToolHandlerWithMRTR(ConfirmThen(...))` under go-sdk's
    /// server multi-round-trip middleware.
    async fn run_tool(
        &self,
        tool: &tools::Tool,
        arguments: Option<&str>,
        responses: Option<Map<String, Value>>,
        mrtr: bool,
    ) -> Result<Value, RpcError> {
        let bound = match tool.bind(arguments) {
            Ok(b) => b,
            Err(text) => return Ok(tools::text_result(&text, true)),
        };
        let input = bound.input.to_string();
        let log_mrtr = || log::info(tool.name, &[("input", &input), ("mrtr", "true")]);
        let confirm = youtube_core::confirm::printable(&bound.invoke.confirm());
        if !confirm.is_empty() {
            let responses = match responses {
                Some(r) => r,
                None if mrtr => {
                    log_mrtr();
                    return Ok(input_required(&confirm));
                }
                None => {
                    log_mrtr();
                    let answer = self.elicit(&confirm).await.map_err(|e| RpcError {
                        message: format!(
                            "multi-round-trip: fulfilling input request \"confirm\": {}",
                            e.message
                        ),
                        ..e
                    })?;
                    Map::from_iter([("confirm".to_string(), answer)])
                }
            };
            // A response carrying `roots` decodes as a ListRootsResult, not
            // an ElicitResult, so it never confirms.
            let accepted = responses
                .get("confirm")
                .filter(|r| r.get("roots").is_none())
                .and_then(|r| r.get("action"))
                .and_then(Value::as_str)
                == Some("accept");
            if !accepted {
                log_mrtr();
                return Ok(tools::text_result("canceled by user", true));
            }
        }
        match tools::run(bound.invoke.as_ref(), self.server.provider.as_ref()).await {
            Ok(text) => {
                let len = text.len().to_string();
                log::info(tool.name, &[("input", &input), ("output_length", &len)]);
                Ok(tools::text_result(&text, false))
            }
            Err(e) => {
                log::error(&e, &[("tool", tool.name), ("input", input.as_str())]);
                Ok(tools::text_result(&e, true))
            }
        }
    }

    /// go-sdk's `ServerSession.Elicit`: the client's `ElicitResult`.
    async fn elicit(&self, message: &str) -> Result<Value, RpcError> {
        let caps = self
            .client()
            .and_then(|c| c.elicitation)
            .ok_or_else(|| RpcError::plain("client does not support elicitation"))?;
        let present = |k: &str| caps.get(k).is_some_and(|v| !v.is_null());
        if !present("form") && present("url") {
            return Err(RpcError::plain(
                "client does not support \"form\" elicitation",
            ));
        }
        let peer = self
            .peer
            .as_ref()
            .ok_or_else(|| RpcError::plain("client does not support elicitation"))?;
        let res = peer
            .request(
                "elicitation/create",
                json!({"mode": "form", "message": message}),
            )
            .await
            .map_err(|e| RpcError {
                message: format!("calling \"elicitation/create\": {}", e.message),
                ..e
            })?;
        match res {
            Value::Null => Ok(json!({"action": ""})),
            Value::Object(_) => Ok(res),
            other => Err(RpcError::plain(format!(
                "calling \"elicitation/create\": json: cannot unmarshal {} into Go value of type mcp.ElicitResult",
                kind(&other)
            ))),
        }
    }

    async fn read_resource(&self, uri: &str) -> Reply {
        match resources::read(uri, self.server.provider.as_ref()).await {
            Ok((mut result, name)) => {
                log::info("resource read", &[("resource", name), ("uri", uri)]);
                if self.speaks_mrtr() {
                    result["resultType"] = json!("complete");
                }
                reply(result)
            }
            Err(ReadError::NotFound) => Err(RpcError {
                code: INVALID_PARAMS,
                message: "Resource not found".into(),
                data: Some(json!({"uri": uri})),
            }),
            Err(ReadError::Op(e)) => {
                log::error(&e, &[("uri", uri)]);
                Err(RpcError::plain(e))
            }
        }
    }
}

fn capabilities() -> Value {
    json!({
        "logging": {},
        "resources": {"listChanged": true},
        "tools": {"listChanged": true},
    })
}

/// go-sdk's `validateRequestMeta`: the client a new-protocol call describes
/// in its `_meta`, or `None` for an old-protocol call.
fn request_meta(raw: Option<&str>) -> Result<Option<Client>, RpcError> {
    #[derive(Deserialize)]
    struct Params {
        #[serde(rename = "_meta")]
        meta: Option<Map<String, Value>>,
    }
    let Some(meta) = raw
        .and_then(|r| serde_json::from_str::<Params>(r).ok())
        .and_then(|p| p.meta)
    else {
        return Ok(None);
    };
    let version = match meta.get(META_PROTOCOL_VERSION) {
        Some(Value::String(v)) if v.as_str() >= MRTR_VERSION => v.clone(),
        _ => return Ok(None),
    };
    let invalid = |key: &str, prefix: &str| {
        RpcError::new(
            INVALID_PARAMS,
            format!("{prefix}invalid _meta field {}", go_quote(key)),
        )
    };
    if !matches!(
        meta.get(META_CLIENT_INFO),
        None | Some(Value::Null | Value::Object(_))
    ) {
        return Err(invalid(META_CLIENT_INFO, ""));
    }
    let Some(Value::Object(caps)) = meta.get(META_CLIENT_CAPABILITIES) else {
        return Err(invalid(META_CLIENT_CAPABILITIES, "missing or "));
    };
    Ok(Some(Client {
        requested_version: version,
        elicitation: caps.get("elicitation").and_then(Value::as_object).cloned(),
    }))
}

/// go-sdk's `negotiatedVersion`: the client's version when supported and
/// older than 2026-07-28, else 2025-11-25.
fn negotiate(requested: &str) -> &'static str {
    PROTOCOL_VERSIONS
        .iter()
        .find(|v| **v == requested && requested < MRTR_VERSION)
        .copied()
        .unwrap_or("2025-11-25")
}

fn input_required(message: &str) -> Value {
    json!({
        "content": null,
        "requestState": "awaiting_confirm",
        "resultType": "input_required",
        "inputRequests": {"confirm": {
            "method": "elicitation/create",
            "params": {"mode": "form", "message": message},
        }},
    })
}

#[derive(Deserialize)]
struct CallArguments<'a> {
    #[serde(borrow, default, deserialize_with = "present_ref")]
    arguments: Option<&'a RawValue>,
}

fn present_ref<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<&'de RawValue>, D::Error> {
    <&RawValue>::deserialize(d).map(Some)
}

/// go-sdk's `InputResponseMap` decoding, which picks the response type by
/// its first key among `roots`, `action` and `role`. A present `null`
/// decodes to an empty map, which still counts as answered.
fn input_responses(params: &Value, raw: &str) -> Result<Option<Map<String, Value>>, RpcError> {
    let Some(responses) = params.get("inputResponses") else {
        return Ok(None);
    };
    let map = match responses {
        Value::Null => Map::new(),
        Value::Object(m) => m.clone(),
        other => {
            return Err(params_error(
                raw,
                &format!(
                    "json: cannot unmarshal {} into Go value of type map[string]json.RawMessage",
                    kind(other)
                ),
            ));
        }
    };
    for (k, v) in &map {
        let has = |key: &str| v.get(key).is_some();
        let cause = if has("roots") || has("action") {
            continue;
        } else if has("role") {
            if v.get("content").is_some_and(|c| !c.is_null()) {
                continue;
            }
            "nil content"
        } else {
            "cannot determine InputResponse type: expected \"action\", \"role\", or \"roots\" key"
        };
        return Err(params_error(
            raw,
            &format!("inputResponses[{}]: {cause}", go_quote(k)),
        ));
    }
    Ok(Some(map))
}

fn params_error(raw: &str, cause: &str) -> RpcError {
    RpcError::new(
        INVALID_PARAMS,
        format!(
            "handling 'tools/call': invalid params: unmarshaling {} into a *mcp.CallToolParamsRaw: {cause}",
            go_quote(raw)
        ),
    )
}

/// List results: go-sdk pages by 100, so with no cursor everything fits; it
/// issues no cursors, so any cursor a client sends is invalid.
fn paged(params: &Value, all: impl FnOnce() -> String) -> Reply {
    if !str_field(params, "cursor").is_empty() {
        return Err(RpcError::new(INVALID_PARAMS, "invalid params"));
    }
    Ok(all())
}

fn str_field<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// jsonrpc2 runs every call but `initialize` asynchronously; notifications
/// and responses are handled in order.
fn is_concurrent(msg: &Message) -> bool {
    matches!(msg, Message::Request { method, .. } if method != "initialize")
}

/// go-sdk tracks a batch's requests by ID, and counts notifications as
/// requests with the empty ID, so two notifications collide.
fn unique_ids(msgs: &[Message]) -> Result<(), String> {
    let mut seen = Vec::new();
    for m in msgs {
        let id = match m {
            Message::Request { id, .. } => id.clone(),
            Message::Notification { .. } => Value::Null,
            Message::Response { .. } => continue,
        };
        if seen.contains(&id) {
            return Err(format!(
                "duplicate message ID {}",
                go_quote(&id.to_string())
            ));
        }
        seen.push(id);
    }
    Ok(())
}

/// Serves one session over stdin and stdout until stdin closes. Requests
/// other than `initialize` run concurrently, as go-sdk runs them, so a tool
/// call waiting on an elicitation does not block the reader that delivers
/// the answer.
pub async fn serve_stdio(server: Arc<Server>) -> Result<(), String> {
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let writer = tokio::spawn(async move {
        let mut out = tokio::io::stdout();
        while let Some(line) = rx.recv().await {
            if log::enabled(Level::Debug) {
                log::raw(&format!("write: {line}"));
            }
            let mut buf = line.into_bytes();
            buf.push(b'\n');
            if out.write_all(&buf).await.is_err() || out.flush().await.is_err() {
                break;
            }
        }
    });
    let session = Session::new(server, Some(Peer::new(tx.clone())));
    let mut tasks = tokio::task::JoinSet::new();
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut frame = String::new();
    let result = loop {
        let line = match lines.next_line().await {
            Ok(Some(l)) => l,
            Ok(None) => break Ok(()),
            Err(e) => break Err(e.to_string()),
        };
        if frame.is_empty() && line.trim().is_empty() {
            continue;
        }
        frame.push_str(&line);
        let incoming = match Incoming::parse(&frame) {
            Ok(m) => m,
            Err(ParseError::Incomplete) => {
                frame.push('\n');
                continue;
            }
            Err(ParseError::Invalid(e)) => break Err(e),
        };
        if log::enabled(Level::Debug) {
            log::raw(&format!("read: {frame}"));
        }
        frame.clear();
        let msgs = match incoming {
            Incoming::Single(msg) => {
                if is_concurrent(&msg) {
                    let (s, tx) = (session.clone(), tx.clone());
                    tasks.spawn(async move {
                        if let Some(r) = s.handle(msg).await {
                            let _ = tx.send(r);
                        }
                    });
                } else if let Some(r) = session.handle(msg).await {
                    let _ = tx.send(r);
                }
                while tasks.try_join_next().is_some() {}
                continue;
            }
            Incoming::Batch(msgs) => msgs,
        };
        // go-sdk's version gate on batches never fires over stdio: its
        // logging wrapper hides the negotiated version from the transport.
        if let Err(e) = unique_ids(&msgs) {
            break Err(e);
        }
        // Replies to a batch go out together, in request order, once all are
        // done. go-sdk waits for a reply to each notification too, which
        // never comes, so a batch holding one is never answered.
        let answered = !msgs
            .iter()
            .any(|m| matches!(m, Message::Notification { .. }));
        let mut slots = Vec::with_capacity(msgs.len());
        for msg in msgs {
            if is_concurrent(&msg) {
                slots.push(Err(msg));
            } else {
                slots.push(Ok(session.handle(msg).await));
            }
        }
        let (s, tx) = (session.clone(), tx.clone());
        tasks.spawn(async move {
            let mut pending = tokio::task::JoinSet::new();
            let mut out: Vec<Option<String>> = vec![None; slots.len()];
            for (i, slot) in slots.into_iter().enumerate() {
                match slot {
                    Ok(r) => out[i] = r,
                    Err(msg) => {
                        let s = s.clone();
                        pending.spawn(async move { (i, s.handle(msg).await) });
                    }
                }
            }
            while let Some(Ok((i, r))) = pending.join_next().await {
                out[i] = r;
            }
            let replies: Vec<String> = out.into_iter().flatten().collect();
            if answered && !replies.is_empty() {
                let _ = tx.send(format!("[{}]", replies.join(",")));
            }
        });
        while tasks.try_join_next().is_some() {}
    };
    if log::enabled(Level::Debug) && result.is_ok() {
        log::raw("read error: EOF");
    }
    // As go-sdk does when the reader stops, in-flight requests are cancelled.
    tasks.shutdown().await;
    drop(session);
    drop(tx);
    let _ = writer.await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiates_like_go_sdk() {
        assert_eq!(negotiate("2025-06-18"), "2025-06-18");
        assert_eq!(negotiate("2024-11-05"), "2024-11-05");
        assert_eq!(negotiate("2026-07-28"), "2025-11-25");
        assert_eq!(negotiate("1999-01-01"), "2025-11-25");
        assert_eq!(negotiate("2099-01-01"), "2025-11-25");
    }
}
