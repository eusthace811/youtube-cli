//! `youtube mcp --mode http`: go-sdk v1.8.0's stateless streamable HTTP
//! transport with JSON responses, behind cobra-mcp's bearer-token routes.
//!
//! Every POST gets a fresh [`Session`] that serves the caller's token only.
//! Over stateless HTTP the server has no back channel, so a gated tool
//! confirms only through multi round-trip, as in Go. Beyond Go, the server
//! times out slow clients and refuses non-loopback `Origin`s (divergence E8).

use crate::log;
use crate::server::{
    self, INVALID_PARAMS, METHOD_NOT_FOUND, MRTR_VERSION, Message, PROTOCOL_VERSIONS, ParseError,
    Server, Session,
};
use crate::verify::{Caller, SCOPES, Verifier};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::{self, HeaderMap, HeaderValue};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use serde_json::{Value, json};
use std::convert::Infallible;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use youtube_core::ops::ServiceProvider;

pub const MAX_BODY: usize = 2 << 20;
const DRAIN_LIMIT: usize = 8 << 20;
/// A client must send its request headers within this time. hyper also
/// starts this timer while a kept-alive connection waits for its next
/// request, so it doubles as the idle timeout.
pub const HEADER_TIMEOUT: Duration = Duration::from_secs(10);
/// A client must finish sending its body within this time.
pub const BODY_TIMEOUT: Duration = Duration::from_secs(30);

const MCP_PATH: &str = "/mcp";
const METADATA_PATH: &str = "/.well-known/oauth-protected-resource";
/// The version go-sdk assumes when a request carries no header.
const DEFAULT_VERSION: &str = "2025-03-26";
/// Batches are refused from this version on.
const NO_BATCH_VERSION: &str = "2025-06-18";
const HEADER_MISMATCH: i64 = -32020;
const MISSING_CLIENT_CAPABILITIES: i64 = -32021;
const UNSUPPORTED_PROTOCOL_VERSION: i64 = -32022;

pub struct Config {
    pub host: String,
    pub port: u16,
    /// `--baseUrl`; empty means `http://<host>:<port>`.
    pub base_url: String,
    pub tokeninfo_url: String,
    pub allowed_clients: Option<Vec<String>>,
    /// The YouTube API base URL tool calls use.
    pub api_base: String,
    pub version: String,
}

pub struct HttpServer {
    listener: TcpListener,
    app: Arc<App>,
}

struct App {
    verifier: Verifier,
    challenge: HeaderValue,
    metadata: Bytes,
    version: String,
}

/// Binds `host:port` the way Go's `net.Listen` does: the first IPv4
/// address `host` resolves to, so `localhost` is `127.0.0.1` only.
async fn listen(host: &str, port: u16) -> io::Result<TcpListener> {
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port)).await?.collect();
    let addr = addrs
        .iter()
        .find(|a| a.is_ipv4())
        .or(addrs.first())
        .copied()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no addresses"))?;
    TcpListener::bind(addr).await
}

impl HttpServer {
    pub async fn bind(cfg: Config) -> io::Result<HttpServer> {
        let listener = listen(&cfg.host, cfg.port).await?;
        let base = if cfg.base_url.is_empty() {
            format!("http://{}:{}", cfg.host, cfg.port)
        } else {
            cfg.base_url
        };
        let metadata_url = format!("{base}{METADATA_PATH}");
        let challenge = format!(
            "Bearer resource_metadata={}, scope={}",
            log::go_quote(&metadata_url),
            log::go_quote(&SCOPES.join(" "))
        );
        let metadata = json!({
            "resource": format!("{base}{MCP_PATH}"),
            "authorization_servers": ["https://accounts.google.com"],
            "scopes_supported": SCOPES,
        });
        let app = App {
            verifier: Verifier::new(cfg.tokeninfo_url, cfg.allowed_clients, cfg.api_base),
            challenge: HeaderValue::from_str(&challenge)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?,
            metadata: Bytes::from(format!("{metadata}\n")),
            version: cfg.version,
        };
        Ok(HttpServer {
            listener,
            app: Arc::new(app),
        })
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Serves until the listener fails.
    pub async fn run(self) -> io::Result<()> {
        let local = self.listener.local_addr()?;
        let local_loopback = local.ip().is_loopback();
        loop {
            let (stream, _) = match self.listener.accept().await {
                Ok(c) => c,
                Err(e) if is_transient(&e) => {
                    // Out of descriptors (EMFILE, ENFILE) persists until
                    // connections close; retrying at once would spin.
                    if e.raw_os_error().is_some_and(|c| c == 24 || c == 23) {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                    continue;
                }
                Err(e) => return Err(e),
            };
            let app = self.app.clone();
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req| {
                    let app = app.clone();
                    async move { Ok::<_, Infallible>(app.route(req, local_loopback).await) }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .timer(TokioTimer::new())
                    .header_read_timeout(HEADER_TIMEOUT)
                    .title_case_headers(true)
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    }
}

fn is_transient(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::Interrupted
            | io::ErrorKind::WouldBlock
    ) || e.raw_os_error().is_some_and(|c| c == 24 || c == 23)
}

type Resp = Response<Full<Bytes>>;
/// A response that ends the request early.
type Refusal = Box<Resp>;

fn response(status: StatusCode, body: impl Into<Bytes>) -> Resp {
    let mut r = Response::new(Full::new(body.into()));
    *r.status_mut() = status;
    r
}

/// Go's `http.Error`.
fn http_error(status: StatusCode, message: &str) -> Resp {
    let mut r = response(status, format!("{message}\n"));
    let h = r.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    r
}

/// go-sdk's `writeJSONRPCError`.
fn rpc_error(status: StatusCode, id: &Value, code: i64, message: &str) -> Resp {
    let body = json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}});
    let mut r = response(status, body.to_string());
    r.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    r
}

/// go-sdk's `util.IsLoopback` on a `host[:port]`.
fn is_loopback(addr: &str) -> bool {
    // net.SplitHostPort, falling back to the whole address: an IPv6 host is
    // bracketed when a port follows, and a bare one has several colons.
    let host = if let Some(rest) = addr.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        match addr.split_once(':') {
            Some((h, p)) if !p.contains(':') => h,
            _ => addr,
        }
    };
    host == "localhost" || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// An `Origin` a page on this machine sends: `http(s)://<loopback>[:port]`.
fn is_loopback_origin(origin: &str) -> bool {
    origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
        .is_some_and(|authority| !authority.contains('/') && is_loopback(authority))
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> &'a str {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}

impl App {
    async fn route(&self, req: Request<Incoming>, local_loopback: bool) -> Resp {
        match req.uri().path() {
            MCP_PATH => {
                if let Some(origin) = req.headers().get(header::ORIGIN) {
                    let origin = String::from_utf8_lossy(origin.as_bytes());
                    if !is_loopback_origin(&origin) {
                        return http_error(
                            StatusCode::FORBIDDEN,
                            &format!(
                                "Forbidden: invalid Origin header {}",
                                log::go_quote(&origin)
                            ),
                        );
                    }
                }
                let auth = req
                    .headers()
                    .get(header::AUTHORIZATION)
                    .map(HeaderValue::as_bytes);
                match self.verifier.verify(auth).await {
                    Ok(caller) => self.mcp(req, caller, local_loopback).await,
                    Err(d) => {
                        let status = StatusCode::from_u16(d.status)
                            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                        let mut r = http_error(status, &d.message);
                        r.headers_mut()
                            .insert(header::WWW_AUTHENTICATE, self.challenge.clone());
                        r
                    }
                }
            }
            METADATA_PATH => self.metadata(req.method()),
            _ => http_error(StatusCode::NOT_FOUND, "404 page not found"),
        }
    }

    /// go-sdk's `ProtectedResourceMetadataHandler`.
    fn metadata(&self, method: &Method) -> Resp {
        let mut r = if method == Method::OPTIONS {
            response(StatusCode::NO_CONTENT, Bytes::new())
        } else if method != Method::GET {
            http_error(StatusCode::METHOD_NOT_ALLOWED, "Method not allowed")
        } else {
            let mut r = response(StatusCode::OK, self.metadata.clone());
            r.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            );
            r
        };
        let h = r.headers_mut();
        h.insert(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_static("*"),
        );
        h.insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, OPTIONS"),
        );
        h.insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("Content-Type"),
        );
        r
    }

    /// go-sdk's `StreamableHTTPHandler.ServeHTTP` for a stateless server.
    async fn mcp(&self, req: Request<Incoming>, caller: Arc<Caller>, local_loopback: bool) -> Resp {
        let headers = req.headers();
        let host = header_str(headers, "host");
        if local_loopback && !is_loopback(host) {
            return http_error(
                StatusCode::FORBIDDEN,
                &format!("Forbidden: invalid Host header {}", log::go_quote(host)),
            );
        }
        let header_version = header_str(headers, "mcp-protocol-version").to_string();
        if !header_version.is_empty()
            && !PROTOCOL_VERSIONS.contains(&header_version.as_str())
            && header_version.as_str() < MRTR_VERSION
        {
            return http_error(
                StatusCode::BAD_REQUEST,
                &format!(
                    "Bad Request: Unsupported protocol version (supported versions: {})",
                    PROTOCOL_VERSIONS.join(",")
                ),
            );
        }
        if req.method() != Method::POST {
            let mut r = http_error(StatusCode::METHOD_NOT_ALLOWED, "Method Not Allowed");
            r.headers_mut()
                .insert(header::ALLOW, HeaderValue::from_static("POST"));
            return r;
        }
        let ctype = header_str(headers, "content-type");
        let base = ctype.split(';').next().unwrap_or("").trim();
        if !base.eq_ignore_ascii_case("application/json") {
            return http_error(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Content-Type must be 'application/json'",
            );
        }
        if !accepts_json_and_stream(headers) {
            return http_error(
                StatusCode::BAD_REQUEST,
                "Accept must contain both 'application/json' and 'text/event-stream'",
            );
        }
        let last_event_id = headers.contains_key("last-event-id");
        let mcp_headers = McpHeaders {
            method: header_str(headers, "mcp-method").to_string(),
            name: header_str(headers, "mcp-name").to_string(),
        };
        let body = match tokio::time::timeout(BODY_TIMEOUT, read_body(req.into_body())).await {
            Err(_) => {
                let mut r = http_error(StatusCode::REQUEST_TIMEOUT, "request body read timed out");
                close(&mut r);
                return r;
            }
            Ok(Err(r)) => return *r,
            Ok(Ok(b)) => b,
        };
        if last_event_id {
            return http_error(
                StatusCode::BAD_REQUEST,
                "can't send Last-Event-ID for POST request",
            );
        }
        let provider: Arc<dyn ServiceProvider> = caller;
        let server = Server::new(provider, self.version.clone());
        post(&body, &header_version, &mcp_headers, server).await
    }
}

/// go-sdk's `streamableServerConn.servePOST`.
async fn post(
    body: &[u8],
    header_version: &str,
    mcp_headers: &McpHeaders,
    server: Arc<Server>,
) -> Resp {
    if body.is_empty() {
        return http_error(StatusCode::BAD_REQUEST, "POST requires a non-empty body");
    }
    let version = if header_version.is_empty() {
        DEFAULT_VERSION
    } else {
        header_version
    };
    let (msgs, is_batch) = match std::str::from_utf8(body)
        .map_err(|e| e.to_string())
        .and_then(|t| server::Incoming::parse(t).map_err(parse_error_text))
    {
        Ok(server::Incoming::Single(m)) => (vec![m], false),
        Ok(server::Incoming::Batch(ms)) => (ms, true),
        Err(e) => {
            return http_error(StatusCode::BAD_REQUEST, &format!("malformed payload: {e}"));
        }
    };
    if is_batch && version >= NO_BATCH_VERSION {
        return http_error(
            StatusCode::BAD_REQUEST,
            &format!(
                "JSON-RPC batching is not supported in {NO_BATCH_VERSION} and later (request version: {version})"
            ),
        );
    }
    let new_protocol = version >= MRTR_VERSION;
    let mut calls = 0;
    let mut has_initialize = false;
    let mut has_request = false;
    for m in &msgs {
        let (id, method, params) = match m {
            Message::Request { id, method, params } => (Some(id), method, params),
            Message::Notification { method, params } => (None, method, params),
            Message::Response { .. } => continue,
        };
        has_request = true;
        if let Err(r) = check_request(id, method, params.as_deref(), version, header_version) {
            return *r;
        }
        if id.is_some() {
            calls += 1;
        }
        has_initialize |= method == "initialize";
    }
    if let [m] = &msgs[..]
        && !is_batch
        && header_version >= MRTR_VERSION
        && let Err(e) = check_mcp_headers(m, mcp_headers)
    {
        let id = match m {
            Message::Request { id, .. } => id.clone(),
            _ => Value::Null,
        };
        return rpc_error(StatusCode::BAD_REQUEST, &id, HEADER_MISMATCH, &e);
    }
    let session = if has_initialize || (new_protocol && has_request) {
        Session::new(server, None)
    } else {
        Session::preinitialized(server, version)
    };
    if calls == 0 {
        for m in msgs {
            session.handle(m).await;
        }
        return response(StatusCode::ACCEPTED, Bytes::new());
    }
    let replies = run_messages(&session, msgs).await;
    let mut r = if let ([reply], false) = (&replies[..], is_batch) {
        let status = if header_version >= MRTR_VERSION {
            error_status(reply)
        } else {
            StatusCode::OK
        };
        response(status, reply.clone())
    } else {
        response(StatusCode::OK, format!("[{}]", replies.join(",")))
    };
    let h = r.headers_mut();
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-transform"),
    );
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    r
}

/// Reads at most [`MAX_BODY`] bytes. An oversized body is still drained up
/// to [`DRAIN_LIMIT`], so the client reads the 413 instead of a reset.
async fn read_body(mut body: Incoming) -> Result<Bytes, Refusal> {
    let mut buf = Vec::new();
    let mut seen = 0usize;
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|e| {
            Box::new(http_error(
                StatusCode::BAD_REQUEST,
                &format!("failed to read body: {e}"),
            ))
        })?;
        let Ok(data) = frame.into_data() else {
            continue;
        };
        seen += data.len();
        if seen > DRAIN_LIMIT {
            break;
        }
        if seen <= MAX_BODY {
            buf.extend_from_slice(&data);
        }
    }
    if seen > MAX_BODY {
        return Err(Box::new(too_large()));
    }
    Ok(Bytes::from(buf))
}

fn too_large() -> Resp {
    let mut r = http_error(
        StatusCode::PAYLOAD_TOO_LARGE,
        &format!("request body exceeds {MAX_BODY} bytes"),
    );
    close(&mut r);
    r
}

fn close(r: &mut Resp) {
    r.headers_mut()
        .insert(header::CONNECTION, HeaderValue::from_static("close"));
}

fn parse_error_text(e: ParseError) -> String {
    match e {
        ParseError::Incomplete => "unexpected end of JSON input".into(),
        ParseError::Invalid(e) => e,
    }
}

/// go-sdk's `streamableAccepts`, over every `Accept` header.
fn accepts_json_and_stream(headers: &HeaderMap) -> bool {
    let (mut json_ok, mut stream_ok) = (false, false);
    for v in headers.get_all(header::ACCEPT) {
        for token in String::from_utf8_lossy(v.as_bytes()).split(',') {
            let base = token
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            match base.as_str() {
                "application/json" | "application/*" => json_ok = true,
                "text/event-stream" | "text/*" => stream_ok = true,
                "*/*" => (json_ok, stream_ok) = (true, true),
                _ => {}
            }
        }
    }
    json_ok && stream_ok
}

/// The checks servePOST makes on each request before dispatching any:
/// go-sdk's `checkRequest`, then the SEP-2575 `_meta` version triple.
fn check_request(
    id: Option<&Value>,
    method: &str,
    params: Option<&serde_json::value::RawValue>,
    version: &str,
    header_version: &str,
) -> Result<(), Refusal> {
    let is_call = id.is_some();
    let bad = |m: String| Err(Box::new(http_error(StatusCode::BAD_REQUEST, &m)));
    let quoted = log::go_quote(method);
    let Some(info) = server::method_info(method) else {
        let message = format!("JSON RPC not handled: {quoted} unsupported");
        if version >= MRTR_VERSION && is_call {
            return Err(Box::new(rpc_error(
                StatusCode::NOT_FOUND,
                id.unwrap_or(&Value::Null),
                METHOD_NOT_FOUND,
                &message,
            )));
        }
        return bad(message);
    };
    if info.notification && is_call {
        return bad(format!("invalid request: unexpected id for {quoted}"));
    }
    if !info.notification && !is_call {
        return bad(format!("invalid request: missing id for {quoted}"));
    }
    if !info.params_optional && params.is_none() {
        return bad("invalid request: missing required \"params\"".into());
    }
    let meta_version = params
        .and_then(|p| serde_json::from_str::<Value>(p.get()).ok())
        .and_then(|p| {
            p.pointer("/_meta/io.modelcontextprotocol~1protocolVersion")
                .and_then(Value::as_str)
                .map(String::from)
        })
        .unwrap_or_default();
    if (is_call && version >= MRTR_VERSION) || !meta_version.is_empty() {
        let id = id.unwrap_or(&Value::Null);
        let fail = |code, message: String| {
            Err(Box::new(rpc_error(
                StatusCode::BAD_REQUEST,
                id,
                code,
                &message,
            )))
        };
        if header_version.is_empty() {
            return fail(
                HEADER_MISMATCH,
                "Mcp-Protocol-Version header is required for requests carrying \"io.modelcontextprotocol/protocolVersion\"".into(),
            );
        }
        if meta_version.is_empty() {
            return fail(
                INVALID_PARAMS,
                "missing or invalid _meta field \"io.modelcontextprotocol/protocolVersion\"".into(),
            );
        }
        if header_version != meta_version {
            return fail(
                HEADER_MISMATCH,
                format!(
                    "Mcp-Protocol-Version header {} does not match request io.modelcontextprotocol/protocolVersion {}",
                    log::go_quote(header_version),
                    log::go_quote(&meta_version)
                ),
            );
        }
    }
    Ok(())
}

/// The `Mcp-Method` and `Mcp-Name` headers a 2026-07-28 request carries.
struct McpHeaders {
    method: String,
    name: String,
}

/// go-sdk's `validateMcpHeaders`. youtube's tools declare no `x-mcp-header`
/// parameters, so only the method and name are checked.
fn check_mcp_headers(m: &Message, h: &McpHeaders) -> Result<(), String> {
    let (method, params) = match m {
        Message::Request { method, params, .. } | Message::Notification { method, params } => {
            (method, params)
        }
        Message::Response { .. } => return Ok(()),
    };
    if h.method.is_empty() {
        return Err("missing required Mcp-Method header".into());
    }
    if h.method != *method {
        return Err(format!(
            "header mismatch: Mcp-Method header value '{}' does not match body value '{method}'",
            h.method
        ));
    }
    let field = match method.as_str() {
        "tools/call" | "prompts/get" => "name",
        "resources/read" => "uri",
        _ => return Ok(()),
    };
    if h.name.is_empty() {
        return Err(format!(
            "missing required Mcp-Name header for method {}",
            log::go_quote(method)
        ));
    }
    let params: Value = params
        .as_deref()
        .and_then(|p| serde_json::from_str(p.get()).ok())
        .unwrap_or(Value::Null);
    let name = match &params {
        Value::Null => Some(""),
        Value::Object(o) => match o.get(field) {
            None | Some(Value::Null) => Some(""),
            Some(Value::String(s)) => Some(s.as_str()),
            Some(_) => None,
        },
        _ => None,
    }
    .ok_or_else(|| {
        format!(
            "failed to extract name from parameters for method {}",
            log::go_quote(method)
        )
    })?;
    if h.name != name {
        return Err(format!(
            "header mismatch: Mcp-Name header value '{}' does not match body value '{name}'",
            h.name
        ));
    }
    Ok(())
}

/// SEP-2575's HTTP status for a protocol-level error reply.
fn error_status(reply: &str) -> StatusCode {
    let code = serde_json::from_str::<Value>(reply)
        .ok()
        .and_then(|v| v.pointer("/error/code").and_then(Value::as_i64));
    match code {
        Some(METHOD_NOT_FOUND) => StatusCode::NOT_FOUND,
        Some(INVALID_PARAMS | UNSUPPORTED_PROTOCOL_VERSION | MISSING_CLIENT_CAPABILITIES) => {
            StatusCode::BAD_REQUEST
        }
        _ => StatusCode::OK,
    }
}

/// Handles one POST's messages: notifications and `initialize` in order,
/// other calls concurrently, as go-sdk's jsonrpc2 does. Replies come back in
/// request order.
async fn run_messages(session: &Arc<Session>, msgs: Vec<Message>) -> Vec<String> {
    let mut pending = tokio::task::JoinSet::new();
    let mut out: Vec<Option<String>> = vec![None; msgs.len()];
    for (i, m) in msgs.into_iter().enumerate() {
        let concurrent = matches!(&m, Message::Request { method, .. } if method != "initialize");
        if concurrent {
            let s = session.clone();
            pending.spawn(async move { (i, s.handle(m).await) });
        } else {
            out[i] = session.handle(m).await;
        }
    }
    while let Some(Ok((i, r))) = pending.join_next().await {
        out[i] = r;
    }
    out.into_iter().flatten().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_hosts_match_go_sdk() {
        for h in [
            "localhost",
            "localhost:8216",
            "127.0.0.1:1",
            "[::1]:8216",
            "[::1]",
            "::1",
            "127.9.9.9",
        ] {
            assert!(is_loopback(h), "{h}");
        }
        for h in [
            "evil.example",
            "evil.example:8216",
            "10.0.0.1",
            "",
            "localhost.evil",
            "[::2]:1",
        ] {
            assert!(!is_loopback(h), "{h}");
        }
    }

    #[test]
    fn only_loopback_origins_pass() {
        for o in [
            "http://localhost:3000",
            "https://127.0.0.1",
            "http://[::1]:8216",
            "http://[::1]",
        ] {
            assert!(is_loopback_origin(o), "{o}");
        }
        for o in [
            "https://evil.example",
            "null",
            "file://",
            "http://localhost.evil",
            "http://127.0.0.1/x",
        ] {
            assert!(!is_loopback_origin(o), "{o}");
        }
    }
}
