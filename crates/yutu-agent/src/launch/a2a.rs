//! ADK's A2A sublauncher: the public agent card and the JSON-RPC invoke
//! endpoints, `/a2a/v1/invoke` (A2A 1.0) and `/a2a/invoke` (A2A 0.3), with
//! a2a-go's task, event and error shapes. Nobody is there to approve a
//! write, so the agent's writes are declined (divergence E9).

use bytes::Bytes;
use http::Method;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::mpsc;

use super::http::{Req, Resp};
use super::session::Input;
use super::web::App;
use super::wire;
use crate::agent;
use crate::gojson::{encode, sorted};

const EXTENSION: &str = "https://google.github.io/adk-docs/a2a/a2a-extension/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    /// A2A 0.3 at `/a2a/invoke`.
    V0,
    /// A2A 1.0 at `/a2a/v1/invoke`.
    V1,
}

#[derive(Clone)]
enum A2aPart {
    Text(String),
    Data { data: Value, metadata: Value },
}

#[derive(Clone)]
struct Message {
    id: String,
    user: bool,
    parts: Vec<A2aPart>,
    context: Option<String>,
}

#[derive(Clone)]
struct Task {
    id: String,
    context: String,
    history: Vec<Message>,
    artifact: Option<(String, Vec<A2aPart>)>,
    state: &'static str,
    timestamp: Option<String>,
    metadata: Map<String, Value>,
}

#[derive(Default)]
pub struct Tasks(Mutex<HashMap<String, Task>>);

/// ADK's `replacePronouns`: the instruction retold in the first person.
fn first_person(instruction: &str) -> String {
    const SUBS: [(&str, &str); 7] = [
        ("you were", "I was"),
        ("you are", "I am"),
        ("you're", "I am"),
        ("you've", "I have"),
        ("yours", "mine"),
        ("your", "my"),
        ("you", "I"),
    ];
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut text = instruction.to_string();
    for (from, to) in SUBS {
        let mut out = String::with_capacity(text.len());
        let (bytes, n) = (text.as_bytes(), from.len());
        let mut i = 0;
        while i < bytes.len() {
            let hit = i + n <= bytes.len()
                && text.is_char_boundary(i)
                && text.is_char_boundary(i + n)
                && text[i..i + n].eq_ignore_ascii_case(from)
                && (i == 0 || !word(bytes[i - 1]))
                && (i + n == bytes.len() || !word(bytes[i + n]));
            if hit {
                out.push_str(to);
                i += n;
            } else {
                let ch = text[i..].chars().next().expect("in bounds");
                out.push(ch);
                i += ch.len_utf8();
            }
        }
        text = out;
    }
    text
}

/// The card a2a-go's 0.3 compatibility producer serves, plus the bearer
/// scheme every route requires (divergence E11).
pub fn card(agent_url: &str, instruction: &str, google_search: bool) -> String {
    let join = |p: &str| format!("{}{p}", agent_url.trim_end_matches('/'));
    let mut skills = vec![json!({
        "description": format!("{} {}", agent::DESCRIPTION, first_person(instruction)),
        "id": agent::NAME,
        "name": "model",
        "tags": ["llm"],
    })];
    if google_search {
        skills.push(json!({
            "description": "Performs a Google search to retrieve information from the web.",
            "id": format!("{}-{}", agent::NAME, crate::tools::GOOGLE_SEARCH),
            "name": crate::tools::GOOGLE_SEARCH,
            "tags": ["llm", "tools"],
        }));
    }
    let card = json!({
        "defaultInputModes": ["text/plain"],
        "defaultOutputModes": ["text/plain"],
        "description": agent::DESCRIPTION,
        "name": agent::NAME,
        "version": "2.0.0",
        "skills": skills,
        "capabilities": {
            "extensions": [{
                "description": "Content is spread across task artifacts and the status message, which carries long-running function calls.",
                "uri": EXTENSION,
            }],
            "pushNotifications": false,
            "streaming": true,
        },
        "supportedInterfaces": [
            {"url": join("/a2a/v1/invoke"), "protocolBinding": "JSONRPC", "protocolVersion": "1.0"},
            {"url": join("/a2a/invoke"), "protocolBinding": "JSONRPC", "protocolVersion": "0.3"},
        ],
        "url": join("/a2a/invoke"),
        "protocolVersion": "0.3",
        "preferredTransport": "JSONRPC",
        "securitySchemes": {"bearer": {
            "type": "http",
            "scheme": "bearer",
            "description": "The per-launch token yutu agent prints at startup.",
        }},
        "security": [{"bearer": []}],
    });
    crate::gojson::encode_indent(&card)
}

pub fn card_response(card: &str) -> Resp {
    Resp::new(200)
        .header("access-control-allow-origin", "*")
        .header("content-type", "application/json")
        .body(card.to_string())
}

fn error(version: Version, id: &Value, code: i64, message: &str, reason: &str) -> Resp {
    let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let body = json!({"jsonrpc": "2.0", "id": id, "error": {
        "code": code, "message": message,
        "data": [{"@type": "type.googleapis.com/google.rpc.ErrorInfo", "domain": "a2a-protocol.org",
                  "metadata": {"timestamp": timestamp}, "reason": reason}]}});
    // a2a-go's 0.3 handler writes errors without a content type, so
    // net/http sniffs text/plain; the 1.0 handler sets JSON.
    let ct = match version {
        Version::V0 => "text/plain; charset=utf-8",
        Version::V1 => "application/json",
    };
    Resp::new(200)
        .header("content-type", ct)
        .body(format!("{}\n", encode(&body)))
}

fn invalid_params(version: Version, id: &Value, what: &str) -> Resp {
    error(
        version,
        id,
        -32602,
        &format!("{what}: invalid params"),
        "INVALID_PARAMS",
    )
}

fn result(id: &Value, result: Value) -> Resp {
    Resp::new(200)
        .header("content-type", "application/json")
        .body(format!(
            "{}\n",
            encode(&json!({"jsonrpc": "2.0", "id": id, "result": result}))
        ))
}

pub async fn handle(app: &Arc<App>, req: &Req, version: Version) -> Resp {
    let null = Value::Null;
    if req.method != Method::POST {
        return error(version, &null, -32600, "invalid request", "INVALID_REQUEST");
    }
    let body = match req.body() {
        Ok(b) => b,
        Err(e) => {
            return error(
                version,
                &null,
                -32700,
                &format!("parse error: {e}"),
                "PARSE_ERROR",
            );
        }
    };
    let rpc = match wire::decode(body) {
        Ok(v) => v,
        Err(e) => {
            return error(
                version,
                &null,
                -32700,
                &format!("parse error: {e}"),
                "PARSE_ERROR",
            );
        }
    };
    let id = rpc.get("id").cloned().unwrap_or(Value::Null);
    if rpc["jsonrpc"] != "2.0" {
        return error(version, &id, -32600, "invalid request", "INVALID_REQUEST");
    }
    let method = rpc["method"].as_str().unwrap_or_default();
    let params = &rpc["params"];
    let (send, stream, get, cancel) = match version {
        Version::V0 => (
            "message/send",
            "message/stream",
            "tasks/get",
            "tasks/cancel",
        ),
        Version::V1 => (
            "SendMessage",
            "SendStreamingMessage",
            "GetTask",
            "CancelTask",
        ),
    };
    match method {
        m if m == send || m == stream => {
            let Some(raw) = params.get("message").filter(|m| m.is_object()) else {
                return invalid_params(version, &id, "message is required");
            };
            let message = parse_message(raw);
            if message.parts.is_empty() {
                return invalid_params(version, &id, "message parts is required");
            }
            if m == stream {
                send_streaming(app, version, id, message, req.start)
            } else {
                let task = run_task(app, message, &mut |_| {}).await;
                let rendered = task_json(&task, version);
                let res = match version {
                    Version::V0 => rendered,
                    Version::V1 => json!({ "task": rendered }),
                };
                result(&id, res)
            }
        }
        m if m == get => {
            let task_id = params["id"].as_str().unwrap_or_default();
            match app.tasks.0.lock().unwrap().get(task_id) {
                Some(t) => {
                    let rendered = task_json(t, version);
                    result(&id, rendered)
                }
                None => error(
                    version,
                    &id,
                    -32001,
                    "failed to get task: task not found",
                    "TASK_NOT_FOUND",
                ),
            }
        }
        m if m == cancel => {
            let task_id = params["id"].as_str().unwrap_or_default();
            if app.tasks.0.lock().unwrap().contains_key(task_id) {
                error(
                    version,
                    &id,
                    -32002,
                    "failed to cancel: task cannot be canceled",
                    "TASK_NOT_CANCELABLE",
                )
            } else {
                error(
                    version,
                    &id,
                    -32001,
                    "failed to cancel: cancelation failed: canceler setup failed: failed to load a task: task not found",
                    "TASK_NOT_FOUND",
                )
            }
        }
        _ => error(version, &id, -32601, "method not found", "METHOD_NOT_FOUND"),
    }
}

fn parse_message(v: &Value) -> Message {
    let parts = v["parts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            if let Some(t) = p.get("text").and_then(Value::as_str) {
                return Some(A2aPart::Text(t.to_string()));
            }
            p.get("data").map(|d| A2aPart::Data {
                data: d.clone(),
                metadata: p.get("metadata").cloned().unwrap_or(Value::Null),
            })
        })
        .collect();
    Message {
        id: v["messageId"].as_str().unwrap_or_default().to_string(),
        user: matches!(v["role"].as_str(), Some("user" | "ROLE_USER")),
        parts,
        context: v["contextId"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(String::from),
    }
}

fn part_json(p: &A2aPart, version: Version) -> Value {
    let mut m = Map::new();
    match p {
        A2aPart::Text(t) => {
            if version == Version::V0 {
                m.insert("kind".into(), "text".into());
            }
            m.insert("text".into(), t.clone().into());
        }
        A2aPart::Data { data, metadata } => {
            if version == Version::V0 {
                m.insert("kind".into(), "data".into());
            }
            m.insert("data".into(), data.clone());
            if !metadata.is_null() {
                m.insert("metadata".into(), metadata.clone());
            }
        }
    }
    Value::Object(m)
}

fn message_json(msg: &Message, version: Version) -> Value {
    let mut m = Map::new();
    if version == Version::V0 {
        m.insert("kind".into(), "message".into());
    }
    if let Some(c) = &msg.context {
        m.insert("contextId".into(), c.clone().into());
    }
    m.insert("messageId".into(), msg.id.clone().into());
    m.insert(
        "parts".into(),
        msg.parts.iter().map(|p| part_json(p, version)).collect(),
    );
    let role = match (version, msg.user) {
        (Version::V0, true) => "user",
        (Version::V0, false) => "agent",
        (Version::V1, true) => "ROLE_USER",
        (Version::V1, false) => "ROLE_AGENT",
    };
    m.insert("role".into(), role.into());
    Value::Object(m)
}

fn state(state: &str, version: Version) -> String {
    match version {
        Version::V0 => state.to_string(),
        Version::V1 => format!("TASK_STATE_{}", state.to_uppercase().replace('-', "_")),
    }
}

fn status(task: &Task, version: Version) -> Value {
    let mut s = Map::new();
    s.insert("state".into(), state(task.state, version).into());
    if let Some(t) = &task.timestamp {
        s.insert("timestamp".into(), t.clone().into());
    }
    Value::Object(s)
}

fn task_json(task: &Task, version: Version) -> Value {
    let mut m = Map::new();
    if version == Version::V0 {
        m.insert("kind".into(), "task".into());
    }
    m.insert("id".into(), task.id.clone().into());
    if let Some((id, parts)) = &task.artifact {
        m.insert(
            "artifacts".into(),
            json!([{"artifactId": id, "parts": parts.iter().map(|p| part_json(p, version)).collect::<Vec<_>>()}]),
        );
    }
    m.insert("contextId".into(), task.context.clone().into());
    m.insert(
        "history".into(),
        task.history
            .iter()
            .map(|h| message_json(h, version))
            .collect(),
    );
    m.insert("metadata".into(), Value::Object(task.metadata.clone()));
    m.insert("status".into(), status(task, version));
    Value::Object(m)
}

/// ADK's event metadata: who wrote it and where, sorted as a Go map.
fn invocation_meta(app_user: &str, session: &str) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("adk_app_name".into(), agent::NAME.into());
    m.insert("adk_session_id".into(), session.into());
    m.insert("adk_user_id".into(), app_user.into());
    m
}

/// One stream update, already rendered for both protocol versions.
enum Update {
    Task(Task),
    Working(Task),
    Artifact {
        task: Task,
        append: bool,
        parts: Vec<A2aPart>,
        metadata: Value,
    },
    Final(Task),
}

/// Runs the agent for one message and returns the finished task; every
/// update along the way goes to `on_update`.
async fn run_task(
    app: &Arc<App>,
    message: Message,
    on_update: &mut (dyn FnMut(Update) + Send),
) -> Task {
    let context = message.context.clone().unwrap_or_else(wire::uuid7);
    let user = format!("A2A_USER_{context}");
    let mut task = Task {
        id: wire::uuid7(),
        context: context.clone(),
        history: vec![message.clone()],
        artifact: None,
        state: "submitted",
        timestamp: None,
        metadata: Map::from_iter([(EXTENSION.to_string(), Value::Bool(true))]),
    };
    on_update(Update::Task(task.clone()));
    let session = match app.store.get(agent::NAME, &user, &context) {
        Ok(s) => Ok(s),
        Err(_) => app
            .store
            .create(agent::NAME, &user, Some(&context), Map::new()),
    };
    let mut meta = invocation_meta(&user, &context);
    meta.insert(EXTENSION.into(), true.into());
    task.metadata = meta.clone();
    task.state = "working";
    task.timestamp = Some(wire::now_rfc3339());
    on_update(Update::Working(task.clone()));
    let text: Vec<Value> = message
        .parts
        .iter()
        .filter_map(|p| match p {
            A2aPart::Text(t) => Some(json!({"text": t})),
            A2aPart::Data { .. } => None,
        })
        .collect();
    let input = Input {
        message: json!({"role": "user", "parts": text}),
        state_delta: None,
        stream: false,
        human: false,
    };
    let artifact_id = wire::uuid7();
    let mut events: Vec<Value> = Vec::new();
    let outcome = match session {
        Ok(s) => s.run(&app.agent, input, &mut |e| events.push(e)).await,
        Err(e) => Err(e),
    };
    for e in &events {
        let parts = event_parts(e);
        if parts.is_empty() || e.get("partial").is_some() {
            continue;
        }
        let mut m = invocation_meta(&user, &context);
        m.insert("adk_author".into(), e["author"].clone());
        m.insert("adk_invocation_id".into(), e["invocationId"].clone());
        m.insert("adk_partial".into(), false.into());
        if !e["usageMetadata"].is_null() {
            m.insert("adk_usage_metadata".into(), e["usageMetadata"].clone());
        }
        let append = task.artifact.is_some();
        match &mut task.artifact {
            Some((_, all)) => all.extend(parts.iter().cloned()),
            None => task.artifact = Some((artifact_id.clone(), parts.clone())),
        }
        on_update(Update::Artifact {
            task: task.clone(),
            append,
            parts,
            metadata: sorted(&Value::Object(m)),
        });
    }
    task.timestamp = Some(wire::now_rfc3339());
    match outcome {
        Ok(()) => task.state = "completed",
        Err(e) => {
            task.state = "failed";
            task.history.push(Message {
                id: wire::uuid4(),
                user: false,
                parts: vec![A2aPart::Text(e)],
                context: Some(context.clone()),
            });
        }
    }
    app.tasks
        .0
        .lock()
        .unwrap()
        .insert(task.id.clone(), task.clone());
    on_update(Update::Final(task.clone()));
    task
}

/// ADK's `ToA2AParts` for one event's content.
fn event_parts(e: &Value) -> Vec<A2aPart> {
    let long_running: Vec<&Value> = e["longRunningToolIds"]
        .as_array()
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    e["content"]["parts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            if let Some(t) = p.get("text").and_then(Value::as_str) {
                return Some(A2aPart::Text(t.to_string()));
            }
            if let Some(c) = p.get("functionCall") {
                return Some(A2aPart::Data {
                    data: sorted(c),
                    metadata: json!({"adk_is_long_running": long_running.contains(&&c["id"]),
                                     "adk_type": "function_call"}),
                });
            }
            p.get("functionResponse").map(|r| A2aPart::Data {
                data: sorted(r),
                metadata: json!({"adk_type": "function_response"}),
            })
        })
        .collect()
}

fn update_json(u: &Update, version: Version) -> Value {
    match u {
        Update::Task(t) => {
            let rendered = task_json(t, version);
            match version {
                Version::V0 => rendered,
                Version::V1 => json!({ "task": rendered }),
            }
        }
        Update::Working(t) | Update::Final(t) => {
            let mut m = Map::new();
            if version == Version::V0 {
                m.insert("kind".into(), "status-update".into());
            }
            m.insert("contextId".into(), t.context.clone().into());
            if version == Version::V0 {
                m.insert("final".into(), matches!(u, Update::Final(_)).into());
            }
            m.insert("status".into(), status(t, version));
            m.insert("taskId".into(), t.id.clone().into());
            m.insert("metadata".into(), Value::Object(t.metadata.clone()));
            match version {
                Version::V0 => Value::Object(m),
                Version::V1 => json!({ "statusUpdate": m }),
            }
        }
        Update::Artifact {
            task,
            append,
            parts,
            metadata,
        } => {
            let mut m = Map::new();
            if version == Version::V0 {
                m.insert("kind".into(), "artifact-update".into());
            }
            if *append {
                m.insert("append".into(), true.into());
            }
            let id = task
                .artifact
                .as_ref()
                .map(|(id, _)| id.clone())
                .unwrap_or_default();
            m.insert(
                "artifact".into(),
                json!({"artifactId": id, "parts": parts.iter().map(|p| part_json(p, version)).collect::<Vec<_>>()}),
            );
            m.insert("contextId".into(), task.context.clone().into());
            m.insert("taskId".into(), task.id.clone().into());
            m.insert("metadata".into(), metadata.clone());
            match version {
                Version::V0 => Value::Object(m),
                Version::V1 => json!({ "artifactUpdate": m }),
            }
        }
    }
}

fn send_streaming(
    app: &Arc<App>,
    version: Version,
    id: Value,
    message: Message,
    start: Instant,
) -> Resp {
    let (tx, rx) = mpsc::unbounded_channel::<Bytes>();
    let deadline = app.cfg.write_timeout().map(|t| start + t);
    let app = app.clone();
    tokio::spawn(async move {
        run_task(&app, message, &mut |u| {
            let data =
                encode(&json!({"jsonrpc": "2.0", "id": id, "result": update_json(&u, version)}));
            let _ = tx.send(format!("id: {}\ndata: {data}\n\n", wire::uuid4()).into());
        })
        .await;
    });
    Resp::stream(200, rx, deadline)
        .header("cache-control", "no-cache")
        .header("connection", "keep-alive")
        .header("content-type", "text/event-stream")
        .header("x-accel-buffering", "no")
}

#[cfg(test)]
mod tests {
    use super::first_person;

    #[test]
    fn instructions_turn_first_person_like_adk() {
        assert_eq!(
            first_person("You are kind. Your tools are yours; you've used you."),
            "I am kind. my tools are mine; I have used I."
        );
        assert_eq!(first_person("youth layout"), "youth layout");
    }
}
