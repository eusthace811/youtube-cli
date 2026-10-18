//! ADK's in-memory session service and runner for the web launchers.
//!
//! A session keeps two views of one conversation: the ADK events the REST
//! API returns, and the model history `Agent::turn` extends. A turn runs on
//! its own task so it can outlive the request that started it: when a write
//! needs a human (divergence E9) the turn pauses at ADK's tool confirmation
//! request, the request ends, and the next request on the session answers
//! it and resumes the turn.

use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

use super::wire;
use crate::agent::{self, Agent, BoxFuture, Frontend, Verdict};
use crate::gojson::sorted;
use crate::llm::Meta;
use crate::model::{Content, FunctionCall, FunctionResponse, Part, Role};
use crate::tools::Approval;

/// ADK's `toolconfirmation.FunctionCallName`.
pub const CONFIRMATION: &str = "adk_request_confirmation";

type Key = (String, String, String);

#[derive(Default)]
pub struct Store {
    sessions: Mutex<BTreeMap<Key, Arc<Session>>>,
}

pub struct Session {
    pub app: String,
    pub user: String,
    pub id: String,
    data: Mutex<Data>,
    turn: tokio::sync::Mutex<Slot>,
    /// Set once the session is deleted; a turn still running stops at its
    /// next approval instead of waiting for an answer nobody can send.
    deleted: std::sync::atomic::AtomicBool,
}

struct Data {
    state: Map<String, Value>,
    events: Vec<Value>,
    updated: i64,
}

/// Where a session's model history lives: here between turns, or inside
/// the task running one, which reports on `rx` and may wait on `pending`.
enum Slot {
    Idle(Vec<Content>),
    Busy {
        rx: mpsc::UnboundedReceiver<Msg>,
        pending: Option<Pending>,
    },
}

/// A write waiting for the human: the confirmation request's call id and
/// the paused turn's answer channel.
struct Pending {
    call_id: String,
    answer: oneshot::Sender<Verdict>,
}

enum Msg {
    Event(Value),
    Paused(Pending),
    Done(Vec<Content>, Result<(), String>),
}

/// ADK's in-memory session service error for missing identifiers.
fn required(app: &str, user: &str, id: &str) -> String {
    use yutu_auth::log::quote;
    format!(
        "app_name, user_id, session_id are required, got app_name: {}, user_id: {}, session_id: {}",
        quote(app),
        quote(user),
        quote(id)
    )
}

impl Store {
    pub fn create(
        &self,
        app: &str,
        user: &str,
        id: Option<&str>,
        state: Map<String, Value>,
    ) -> Result<Arc<Session>, String> {
        if app.is_empty() || user.is_empty() {
            return Err(format!(
                "app_name and user_id are required, got app_name: {}, user_id: {}",
                yutu_auth::log::quote(app),
                yutu_auth::log::quote(user)
            ));
        }
        let id = id
            .filter(|s| !s.is_empty())
            .map(String::from)
            .unwrap_or_else(wire::uuid4);
        let mut sessions = self.sessions.lock().unwrap();
        let key = (app.to_string(), user.to_string(), id.clone());
        if sessions.contains_key(&key) {
            return Err(format!("session {id} already exists"));
        }
        let s = Arc::new(Session {
            app: app.into(),
            user: user.into(),
            id,
            data: Mutex::new(Data {
                state,
                events: Vec::new(),
                updated: wire::unix_now(),
            }),
            turn: tokio::sync::Mutex::new(Slot::Idle(Vec::new())),
            deleted: std::sync::atomic::AtomicBool::new(false),
        });
        sessions.insert(key, s.clone());
        Ok(s)
    }

    pub fn get(&self, app: &str, user: &str, id: &str) -> Result<Arc<Session>, String> {
        if app.is_empty() || user.is_empty() || id.is_empty() {
            return Err(required(app, user, id));
        }
        let key = (app.to_string(), user.to_string(), id.to_string());
        self.sessions
            .lock()
            .unwrap()
            .get(&key)
            .cloned()
            .ok_or_else(|| format!("session not found: {}", yutu_auth::log::quote(id)))
    }

    pub fn list(&self, app: &str, user: &str) -> Vec<Arc<Session>> {
        self.sessions
            .lock()
            .unwrap()
            .iter()
            .filter(|((a, u, _), _)| a == app && u == user)
            .map(|(_, s)| s.clone())
            .collect()
    }

    /// Removes a session and stops its turn, so the task and history do
    /// not outlive it: dropping the turn's channels stops a paused turn,
    /// discards one queued to pause, and makes a running one stop at its
    /// next approval.
    pub fn delete(&self, app: &str, user: &str, id: &str) {
        let key = (app.to_string(), user.to_string(), id.to_string());
        let removed = self.sessions.lock().unwrap().remove(&key);
        if let Some(session) = removed {
            session
                .deleted
                .store(true, std::sync::atomic::Ordering::SeqCst);
            if let Ok(mut slot) = session.turn.try_lock() {
                *slot = Slot::Idle(Vec::new());
            }
        }
    }
}

/// What a run starts from: the request's message and options.
pub struct Input {
    /// `newMessage` as the client sent it, kept for the user event.
    pub message: Value,
    pub state_delta: Option<Map<String, Value>>,
    pub stream: bool,
    /// A human can answer tool confirmations (the web UI). Without one every
    /// write is declined with the cancellation result.
    pub human: bool,
}

impl Session {
    /// ADK's session JSON; `full` keeps the events, as Get does and List
    /// does not.
    pub fn json(&self, full: bool) -> Value {
        let d = self.data.lock().unwrap();
        json!({
            "id": self.id,
            "appName": self.app,
            "userId": self.user,
            "lastUpdateTime": d.updated,
            "events": if full { d.events.clone() } else { Vec::new() },
            "state": sorted(&Value::Object(d.state.clone())),
        })
    }

    pub fn has_event(&self, id: &str) -> bool {
        self.data
            .lock()
            .unwrap()
            .events
            .iter()
            .any(|e| e["id"] == id)
    }

    pub fn state(&self) -> Map<String, Value> {
        self.data.lock().unwrap().state.clone()
    }

    fn append(&self, event: Value) {
        let mut d = self.data.lock().unwrap();
        if let Some(delta) = event["actions"]["stateDelta"].as_object() {
            for (k, v) in delta {
                d.state.insert(k.clone(), v.clone());
            }
        }
        d.events.push(event);
        d.updated = wire::unix_now();
    }

    /// PATCH's state update: a user event carrying only the delta.
    pub fn update_state(&self, delta: Map<String, Value>) {
        self.append(event(
            &wire::uuid4(),
            "user",
            None,
            None,
            Actions {
                state_delta: delta,
                ..Actions::default()
            },
        ));
    }

    /// Runs one message to the end of the turn or to a pause for approval,
    /// handing each event to `out` as it happens.
    pub async fn run(
        self: &Arc<Self>,
        agent: &Arc<Agent>,
        input: Input,
        out: &mut (dyn FnMut(Value) + Send),
    ) -> Result<(), String> {
        let mut slot = self.turn.lock().await;
        let answer = confirmation_answer(&input.message);
        // A turn left paused or running by an earlier request settles first:
        // the matching answer resumes it, anything else stops it.
        let mut history = loop {
            let pending = match &mut *slot {
                Slot::Idle(history) => break std::mem::take(history),
                Slot::Busy { pending, .. } => pending.take(),
            };
            match (pending, &answer) {
                // Only the person at the browser may answer; a program's
                // answer stops the turn like any other message.
                (Some(p), Some((id, confirmed))) if *id == p.call_id && input.human => {
                    self.append(user_event(&wire::uuid4(), &input));
                    let verdict = if *confirmed {
                        Verdict::Approve
                    } else {
                        Verdict::Decline
                    };
                    let _ = p.answer.send(verdict);
                    return drain(&mut slot, out).await;
                }
                (pending, _) => {
                    if let Some(p) = pending {
                        let _ = p.answer.send(Verdict::Stop);
                    }
                    let _ = drain(&mut slot, &mut |_| {}).await;
                }
            }
        };
        if answer.is_some() {
            // An answer to a request no turn is waiting on: kept, not run.
            *slot = Slot::Idle(history);
            self.append(user_event(&wire::uuid4(), &input));
            return Ok(());
        }
        let invocation = format!("e-{}", wire::uuid4());
        let user = user_event(&invocation, &input);
        self.append(user);
        let message = content_from_json(&input.message);
        let (tx, rx) = mpsc::unbounded_channel();
        let mut front = Web {
            tx: tx.clone(),
            session: self.clone(),
            invocation,
            human: input.human,
            stream: input.stream,
            ids: Vec::new(),
        };
        let agent = agent.clone();
        let stream = input.stream;
        tokio::spawn(async move {
            let result = agent.turn(&mut history, message, stream, &mut front).await;
            let _ = tx.send(Msg::Done(history, result));
        });
        *slot = Slot::Busy { rx, pending: None };
        drain(&mut slot, out).await
    }
}

/// Forwards a running turn's events until it pauses or ends.
async fn drain(slot: &mut Slot, out: &mut (dyn FnMut(Value) + Send)) -> Result<(), String> {
    let Slot::Busy { rx, pending } = slot else {
        return Ok(());
    };
    while let Some(msg) = rx.recv().await {
        match msg {
            Msg::Event(v) => out(v),
            Msg::Paused(p) => {
                *pending = Some(p);
                return Ok(());
            }
            Msg::Done(history, result) => {
                *slot = Slot::Idle(history);
                return result;
            }
        }
    }
    *slot = Slot::Idle(Vec::new());
    Err("the agent stopped before finishing the turn".into())
}

/// The confirmation the UI sends: `(call id, confirmed)` when the message
/// is a function response to `adk_request_confirmation`.
fn confirmation_answer(message: &Value) -> Option<(String, bool)> {
    let parts = message["parts"].as_array()?;
    parts.iter().find_map(|p| {
        let r = p.get("functionResponse")?;
        (r["name"] == CONFIRMATION).then(|| {
            let resp = &r["response"];
            // ADK also accepts the confirmation JSON-encoded under "response".
            let confirmed = match resp.get("response").and_then(Value::as_str) {
                Some(s) => serde_json::from_str::<Value>(s)
                    .ok()
                    .map(|v| v["confirmed"] == true),
                None => Some(resp["confirmed"] == true),
            };
            (
                r["id"].as_str().unwrap_or_default().to_string(),
                confirmed.unwrap_or(false),
            )
        })
    })
}

/// The model's view of a genai `Content` the client sent.
pub fn content_from_json(v: &Value) -> Content {
    let role = if v["role"] == "model" {
        Role::Model
    } else {
        Role::User
    };
    let parts = v["parts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            if let Some(t) = p.get("text").and_then(Value::as_str) {
                return Some(Part::Text {
                    text: t.to_string(),
                    thought: p["thought"] == true,
                    signature: None,
                });
            }
            if let Some(r) = p.get("functionResponse") {
                return Some(Part::Response(FunctionResponse {
                    id: r["id"].as_str().map(String::from),
                    name: r["name"].as_str().unwrap_or_default().into(),
                    response: r["response"].as_object().cloned().unwrap_or_default(),
                }));
            }
            None
        })
        .collect();
    Content { role, parts }
}

#[derive(Default)]
struct Actions {
    state_delta: Map<String, Value>,
    skip_summarization: bool,
    confirmations: Option<Value>,
}

/// One ADK event in Go's field order. Agent events carry `nodeInfo`.
fn event(
    invocation: &str,
    author: &str,
    content: Option<Value>,
    meta: Option<(&Meta, bool)>,
    actions: Actions,
) -> Value {
    let mut e = Map::new();
    e.insert("id".into(), wire::uuid4().into());
    e.insert("invocationId".into(), invocation.into());
    e.insert("author".into(), author.into());
    if let Some((_, true)) = meta {
        e.insert("partial".into(), true.into());
    }
    e.insert("groundingMetadata".into(), Value::Null);
    let m = meta.map(|(m, _)| m);
    e.insert(
        "usageMetadata".into(),
        m.and_then(|m| m.usage.as_ref())
            .map(sorted)
            .unwrap_or(Value::Null),
    );
    if let Some(m) = m {
        if m.turn_complete {
            e.insert("turnComplete".into(), true.into());
        }
        if let Some(f) = &m.finish_reason {
            e.insert("finishReason".into(), f.clone().into());
        }
        if let Some(v) = &m.model_version {
            e.insert("modelVersion".into(), v.clone().into());
        }
    }
    let mut a = Map::new();
    a.insert(
        "stateDelta".into(),
        sorted(&Value::Object(actions.state_delta)),
    );
    a.insert("artifactDelta".into(), json!({}));
    if actions.skip_summarization {
        a.insert("skipSummarization".into(), true.into());
    }
    if let Some(c) = actions.confirmations {
        a.insert("requestedToolConfirmations".into(), c);
    }
    e.insert("actions".into(), Value::Object(a));
    if author != "user" {
        e.insert("nodeInfo".into(), json!({"path": author}));
    }
    e.insert("content".into(), content.unwrap_or(Value::Null));
    Value::Object(e)
}

fn user_event(invocation: &str, input: &Input) -> Value {
    event(
        invocation,
        "user",
        Some(content_json_from_request(&input.message)),
        None,
        Actions {
            state_delta: input.state_delta.clone().unwrap_or_default(),
            ..Actions::default()
        },
    )
}

/// The client's content as Go re-marshals a `genai.Content`.
fn content_json_from_request(v: &Value) -> Value {
    let parts: Vec<Value> = v["parts"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| {
            let mut out = Map::new();
            if let Some(r) = p.get("functionResponse") {
                let mut fr = Map::new();
                for k in ["id", "name"] {
                    if let Some(s) = r[k].as_str().filter(|s| !s.is_empty()) {
                        fr.insert(k.into(), s.into());
                    }
                }
                if let Some(resp) = r.get("response").filter(|r| r.is_object()) {
                    fr.insert("response".into(), sorted(resp));
                }
                out.insert("functionResponse".into(), Value::Object(fr));
            }
            if let Some(t) = p
                .get("text")
                .and_then(Value::as_str)
                .filter(|t| !t.is_empty())
            {
                out.insert("text".into(), t.into());
            }
            if p["thought"] == true {
                out.insert("thought".into(), true.into());
            }
            Value::Object(out)
        })
        .collect();
    let mut c = Map::new();
    if let Some(r) = v["role"].as_str().filter(|r| !r.is_empty()) {
        c.insert("role".into(), r.into());
    }
    c.insert("parts".into(), Value::Array(parts));
    Value::Object(c)
}

fn part_json(p: &Part, id: Option<&str>) -> Value {
    let mut out = Map::new();
    match p {
        Part::Text {
            text,
            thought,
            signature,
        } => {
            if !text.is_empty() {
                out.insert("text".into(), text.clone().into());
            }
            if *thought {
                out.insert("thought".into(), true.into());
            }
            if let Some(s) = signature {
                out.insert("thoughtSignature".into(), s.clone().into());
            }
        }
        Part::Call { call, signature } => {
            if let Some(s) = signature {
                out.insert("thoughtSignature".into(), s.clone().into());
            }
            out.insert("functionCall".into(), call_json(call, id));
        }
        Part::Response(r) => {
            let mut fr = Map::new();
            if let Some(id) = id.or(r.id.as_deref()) {
                fr.insert("id".into(), id.into());
            }
            fr.insert("name".into(), r.name.clone().into());
            fr.insert(
                "response".into(),
                sorted(&Value::Object(r.response.clone())),
            );
            out.insert("functionResponse".into(), Value::Object(fr));
        }
    }
    Value::Object(out)
}

fn call_json(call: &FunctionCall, id: Option<&str>) -> Value {
    let mut fc = Map::new();
    if let Some(id) = id.or(call.id.as_deref()) {
        fc.insert("id".into(), id.into());
    }
    fc.insert("name".into(), call.name.clone().into());
    fc.insert("args".into(), sorted(&Value::Object(call.args.clone())));
    Value::Object(fc)
}

fn content_json(c: &Content, ids: &[String]) -> Value {
    let mut calls = 0;
    let mut responses = 0;
    let parts: Vec<Value> = c
        .parts
        .iter()
        .map(|p| {
            let id = match p {
                Part::Call { .. } => {
                    calls += 1;
                    ids.get(calls - 1)
                }
                Part::Response(_) => {
                    responses += 1;
                    ids.get(responses - 1)
                }
                Part::Text { .. } => None,
            };
            part_json(p, id.map(String::as_str))
        })
        .collect();
    let role = match c.role {
        Role::User => "user",
        Role::Model => "model",
    };
    json!({"role": role, "parts": parts})
}

/// The web frontend of a turn: every content becomes an ADK event, stored
/// and sent to the request draining the turn.
struct Web {
    tx: mpsc::UnboundedSender<Msg>,
    session: Arc<Session>,
    invocation: String,
    human: bool,
    stream: bool,
    /// The function call ids of the latest model reply; ADK names calls the
    /// provider left unnamed `adk-<uuid>`, and their responses follow suit.
    ids: Vec<String>,
}

impl Web {
    fn emit(&self, event: Value) {
        self.session.append(event.clone());
        let _ = self.tx.send(Msg::Event(event));
    }
}

impl Frontend for Web {
    fn chunk(&mut self, chunk: &Content, meta: &Meta) {
        if !self.stream || chunk.parts.is_empty() {
            return;
        }
        let ids: Vec<String> = Vec::new();
        let e = event(
            &self.invocation,
            agent::NAME,
            Some(content_json(chunk, &ids)),
            Some((meta, true)),
            Actions::default(),
        );
        let _ = self.tx.send(Msg::Event(e));
    }

    fn content(&mut self, content: &Content, meta: Option<&Meta>) {
        if content.parts.is_empty() {
            return;
        }
        if content.role == Role::Model {
            self.ids = content
                .calls()
                .map(|c| {
                    c.id.clone()
                        .unwrap_or_else(|| format!("adk-{}", wire::uuid4()))
                })
                .collect();
        }
        let e = event(
            &self.invocation,
            agent::NAME,
            Some(content_json(content, &self.ids)),
            meta.map(|m| (m, false)),
            Actions::default(),
        );
        self.emit(e);
    }

    fn approve<'a>(&'a mut self, a: &'a Approval, position: usize) -> BoxFuture<'a, Verdict> {
        Box::pin(async move {
            if !self.human {
                return Verdict::Decline;
            }
            if self
                .session
                .deleted
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return Verdict::Stop;
            }
            let call_id = self.ids.get(position).cloned().unwrap_or_default();
            let mut hint = format!(
                "Please approve or reject the tool call {}() by responding with a FunctionResponse with an expected ToolConfirmation payload.",
                a.tool
            );
            if !a.message.is_empty() {
                hint = format!("{}\n\n{hint}", a.message);
            }
            let confirmation = json!({"hint": hint, "confirmed": false, "payload": null});
            let error = format!(
                "error tool {} requires confirmation, please approve or reject",
                yutu_auth::log::quote(&a.tool)
            );
            let response = json!({"role": "user", "parts": [{"functionResponse": {
                "id": call_id, "name": a.tool, "response": {"error": error}}}]});
            self.emit(event(
                &self.invocation,
                agent::NAME,
                Some(response),
                None,
                Actions {
                    skip_summarization: true,
                    confirmations: Some(json!({ call_id.clone(): confirmation.clone() })),
                    ..Actions::default()
                },
            ));
            let request_id = format!("adk-{}", wire::uuid4());
            let args: Value = serde_json::from_str(&a.args).unwrap_or_else(|_| json!({}));
            let request = json!({"role": "model", "parts": [{"functionCall": {
                "id": request_id,
                "name": CONFIRMATION,
                "args": {
                    "originalFunctionCall": {"id": call_id, "args": sorted(&args), "name": a.tool},
                    "toolConfirmation": confirmation,
                },
            }}]});
            let mut e = event(
                &self.invocation,
                agent::NAME,
                Some(request),
                None,
                Actions::default(),
            );
            e.as_object_mut().expect("object").shift_insert(
                3,
                "longRunningToolIds".into(),
                json!([request_id]),
            );
            self.emit(e);
            let (answer, verdict) = oneshot::channel();
            let _ = self.tx.send(Msg::Paused(Pending {
                call_id: request_id,
                answer,
            }));
            // A dropped answer means the session went away: nothing runs.
            verdict.await.unwrap_or(Verdict::Stop)
        })
    }
}
