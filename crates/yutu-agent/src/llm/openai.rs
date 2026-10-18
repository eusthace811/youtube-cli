//! OpenAI's Responses API (`POST {base}/responses`), the API ADK's openai
//! adapter selects by default, with the request shape it sends and the
//! openai-go client's retry policy.

use reqwest::StatusCode;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderValue};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::time::Duration;
use yutu_auth::Secret;

use super::{OnText, REQUEST_TIMEOUT, join_url, sse};
use crate::gojson;
use crate::model::{Content, FunctionCall, Part, Request, Role, merge};

pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1/";
const MAX_RETRIES: u32 = 2;

pub struct OpenAi {
    http: reqwest::Client,
    base_url: String,
    model: String,
    key: Secret,
    /// `OpenAI-Organization` and `OpenAI-Project`, from the environment as
    /// openai-go reads them.
    org: Option<String>,
    project: Option<String>,
}

impl OpenAi {
    pub fn new(model: &str, key: Secret, base_url: &str) -> OpenAi {
        OpenAi {
            http: super::http_client(),
            base_url: base_url.to_string(),
            model: model.to_string(),
            key,
            org: std::env::var("OPENAI_ORG_ID").ok(),
            project: std::env::var("OPENAI_PROJECT_ID").ok(),
        }
    }

    pub async fn generate(
        &self,
        req: &Request<'_>,
        stream: bool,
        on_text: OnText<'_>,
    ) -> Result<Content, String> {
        let url = join_url(&self.base_url, "responses");
        let mut body = self.body(req);
        if stream {
            body["stream"] = Value::Bool(true);
        }
        let body = body.to_string();
        if !stream {
            let resp = self
                .send(&url, body, stream)
                .await
                .map_err(|e| format!("openai: call failed: {e}"))?;
            let text = resp.text().await.map_err(|e| e.to_string())?;
            let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            return convert(&v);
        }
        let resp = self.send(&url, body, stream).await?;
        read_stream(sse::Reader::new(resp), on_text).await
    }

    async fn send(
        &self,
        url: &str,
        body: String,
        stream: bool,
    ) -> Result<reqwest::Response, String> {
        let mut attempt = 0;
        loop {
            let mut b = self
                .http
                .post(url)
                .header(CONTENT_TYPE, "application/json")
                .header(ACCEPT, "application/json")
                .body(body.clone());
            if !self.key.expose().is_empty() {
                let mut bearer = b"Bearer ".to_vec();
                bearer.extend_from_slice(self.key.expose());
                let mut v = HeaderValue::from_bytes(&bearer).map_err(|e| e.to_string())?;
                v.set_sensitive(true);
                b = b.header(AUTHORIZATION, v);
            }
            if let Some(o) = &self.org {
                b = b.header("OpenAI-Organization", o);
            }
            if let Some(p) = &self.project {
                b = b.header("OpenAI-Project", p);
            }
            if !stream {
                b = b.timeout(REQUEST_TIMEOUT);
            }
            let result = b.send().await;
            let retry = match &result {
                Err(e) => !e.is_builder(),
                Ok(r) => retryable(r.status()),
            };
            if retry && attempt < MAX_RETRIES {
                let wait = result
                    .as_ref()
                    .ok()
                    .and_then(retry_after)
                    .unwrap_or_else(|| Duration::from_millis(500 << attempt));
                attempt += 1;
                tokio::time::sleep(wait).await;
                continue;
            }
            let resp = result.map_err(|e| {
                format!(
                    "Post {}: {}",
                    yutu_auth::log::quote(url),
                    super::describe(&e)
                )
            })?;
            let status = resp.status();
            if status.is_success() {
                return Ok(resp);
            }
            let text = resp.text().await.unwrap_or_default();
            let raw = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v.get("error").map(Value::to_string))
                .unwrap_or(text);
            return Err(format!(
                "POST {}: {} {} {raw}",
                yutu_auth::log::quote(url),
                status.as_u16(),
                status.canonical_reason().unwrap_or_default()
            ));
        }
    }

    fn body(&self, req: &Request<'_>) -> Value {
        let mut input: Vec<Value> = Vec::new();
        for c in req.contents {
            let mut texts: Vec<Value> = Vec::new();
            let flush = |texts: &mut Vec<Value>, input: &mut Vec<Value>| {
                if texts.is_empty() {
                    return;
                }
                let content = Value::Array(std::mem::take(texts));
                input.push(match c.role {
                    Role::User => json!({"type": "message", "role": "user", "content": content}),
                    Role::Model => json!({"type": "message", "role": "assistant",
                                          "status": "completed", "content": content}),
                });
            };
            for p in &c.parts {
                match p {
                    Part::Text { thought: true, .. } => {}
                    Part::Text { text, .. } => {
                        let kind = if c.role == Role::User {
                            "input_text"
                        } else {
                            "output_text"
                        };
                        texts.push(json!({"type": kind, "text": text}));
                    }
                    Part::Call { call, .. } => {
                        flush(&mut texts, &mut input);
                        input.push(json!({
                            "type": "function_call",
                            "call_id": call.id.clone().unwrap_or_default(),
                            "name": call.name,
                            "arguments": gojson::marshal(&Value::Object(call.args.clone())),
                        }));
                    }
                    Part::Response(r) => {
                        flush(&mut texts, &mut input);
                        input.push(json!({
                            "type": "function_call_output",
                            "call_id": r.id.clone().unwrap_or_default(),
                            "output": gojson::marshal(&Value::Object(r.response.clone())),
                        }));
                    }
                }
            }
            flush(&mut texts, &mut input);
        }
        let mut body = json!({"model": self.model, "instructions": req.system, "input": input});
        if !req.declarations.is_empty() {
            let tools: Vec<Value> = req
                .declarations
                .iter()
                .map(|d| {
                    json!({"type": "function", "name": d.name, "description": d.description,
                           "parameters": d.parameters, "strict": false})
                })
                .collect();
            body["tools"] = Value::Array(tools);
        }
        body
    }
}

/// openai-go retries connection errors, 408, 409, 429 and 5xx.
fn retryable(s: StatusCode) -> bool {
    matches!(s.as_u16(), 408 | 409 | 429) || s.is_server_error()
}

fn retry_after(r: &reqwest::Response) -> Option<Duration> {
    let h = r.headers();
    if let Some(ms) = h
        .get("retry-after-ms")
        .and_then(|v| v.to_str().ok()?.parse::<f64>().ok())
    {
        return Some(Duration::from_secs_f64(ms / 1000.0)).filter(|d| d.as_secs() < 60);
    }
    let secs = h.get("retry-after")?.to_str().ok()?.parse::<f64>().ok()?;
    Some(Duration::from_secs_f64(secs)).filter(|d| d.as_secs() < 60)
}

/// A completed Responses API object as genai content.
fn convert(resp: &Value) -> Result<Content, String> {
    if let Some(e) = failure(resp) {
        return Err(e);
    }
    let items = resp["output"].as_array().filter(|i| !i.is_empty());
    let items = items.ok_or("openai: response included no output items")?;
    let mut parts = Vec::new();
    for item in items {
        match item["type"].as_str().unwrap_or_default() {
            "message" => {
                for c in item["content"].as_array().into_iter().flatten() {
                    match c["type"].as_str().unwrap_or_default() {
                        "output_text" => {
                            let t = c["text"].as_str().unwrap_or_default();
                            if !t.is_empty() {
                                parts.push(Part::text(t));
                            }
                        }
                        "refusal" => {
                            parts.push(Part::text(c["refusal"].as_str().unwrap_or_default()))
                        }
                        other => {
                            return Err(format!(
                                "openai: unsupported message content type: {}",
                                yutu_auth::log::quote(other)
                            ));
                        }
                    }
                }
            }
            "function_call" => parts.push(Part::Call {
                call: function_call(
                    item["call_id"].as_str().unwrap_or_default(),
                    item["name"].as_str().unwrap_or_default(),
                    item["arguments"].as_str().unwrap_or_default(),
                )?,
                signature: None,
            }),
            "reasoning" => {
                let chunks = item["content"].as_array().into_iter().flatten();
                let summary = item["summary"].as_array().into_iter().flatten();
                for t in chunks.chain(summary).filter_map(|c| c["text"].as_str()) {
                    if !t.is_empty() {
                        parts.push(Part::Text {
                            text: t.to_string(),
                            thought: true,
                            signature: None,
                        });
                    }
                }
            }
            other => {
                return Err(format!(
                    "openai: unsupported output item type: {}",
                    yutu_auth::log::quote(other)
                ));
            }
        }
    }
    if parts.is_empty() {
        return Err("openai: response output did not contain text or tool content".into());
    }
    Ok(Content {
        role: Role::Model,
        parts,
    })
}

fn failure(resp: &Value) -> Option<String> {
    let status = resp["status"].as_str().unwrap_or_default();
    let msg = resp["error"]["message"].as_str().unwrap_or_default();
    let code = resp["error"]["code"].as_str().unwrap_or_default();
    let failed = status == "failed" || (status.is_empty() && (!msg.is_empty() || !code.is_empty()));
    if !failed {
        return None;
    }
    let mut details = Vec::new();
    if let Some(id) = resp["id"].as_str().filter(|s| !s.is_empty()) {
        details.push(format!("id {}", yutu_auth::log::quote(id)));
    }
    if !code.is_empty() {
        details.push(format!("code {}", yutu_auth::log::quote(code)));
    }
    let base = "openai: response failed";
    Some(match (details.is_empty(), msg.is_empty()) {
        (false, false) => format!(
            "{base} ({}): {}",
            details.join(", "),
            yutu_auth::log::quote(msg)
        ),
        (false, true) => format!("{base} ({})", details.join(", ")),
        (true, false) => format!("{base}: {}", yutu_auth::log::quote(msg)),
        (true, true) => base.to_string(),
    })
}

fn function_call(call_id: &str, name: &str, arguments: &str) -> Result<FunctionCall, String> {
    let raw = if arguments.is_empty() {
        "{}"
    } else {
        arguments
    };
    let args = match serde_json::from_str::<Value>(raw) {
        Ok(Value::Object(m)) => m,
        Ok(Value::Null) => Map::new(),
        Ok(_) | Err(_) => {
            return Err(format!(
                "openai: parse function call args (name {}, call_id {})",
                yutu_auth::log::quote(name),
                yutu_auth::log::quote(call_id)
            ));
        }
    };
    Ok(FunctionCall {
        id: (!call_id.is_empty()).then(|| call_id.to_string()),
        name: name.to_string(),
        args,
    })
}

/// The adapter's stream translation: text and reasoning deltas print as they
/// arrive, calls assemble from their argument deltas, and the terminal
/// response stands in when the deltas carried nothing.
async fn read_stream(mut events: sse::Reader, on_text: OnText<'_>) -> Result<Content, String> {
    let mut parts: Vec<Part> = Vec::new();
    let mut args: HashMap<String, String> = HashMap::new();
    let mut items: HashMap<String, (String, String)> = HashMap::new();
    let mut terminal: Option<Value> = None;
    while let Some(event) = events.next().await? {
        if event.data == "[DONE]" {
            break;
        }
        let e: Value = serde_json::from_str(&event.data).map_err(|e| e.to_string())?;
        let kind = e["type"]
            .as_str()
            .unwrap_or(event.name.as_str())
            .to_string();
        let delta = e["delta"].as_str().unwrap_or_default();
        match kind.as_str() {
            "response.output_text.delta" | "response.refusal.delta" if !delta.is_empty() => {
                on_text(delta);
                push_text(&mut parts, delta, false);
            }
            "response.reasoning_text.delta" | "response.reasoning_summary_text.delta"
                if !delta.is_empty() =>
            {
                on_text(delta);
                push_text(&mut parts, delta, true);
            }
            "response.output_item.added" => {
                let item = &e["item"];
                if let Some(id) = item["id"].as_str() {
                    items.insert(
                        id.to_string(),
                        (
                            item["call_id"].as_str().unwrap_or_default().to_string(),
                            item["name"].as_str().unwrap_or_default().to_string(),
                        ),
                    );
                }
            }
            "response.function_call_arguments.delta" => {
                let id = e["item_id"].as_str().unwrap_or("default").to_string();
                args.entry(id).or_default().push_str(delta);
            }
            "response.function_call_arguments.done" => {
                let id = e["item_id"].as_str().unwrap_or_default();
                let buffered = args.remove(id).unwrap_or_default();
                let payload = e["arguments"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&buffered);
                let (call_id, name) = items
                    .remove(id)
                    .unwrap_or_else(|| (id.to_string(), String::new()));
                parts.push(Part::Call {
                    call: function_call(&call_id, &name, payload)?,
                    signature: None,
                });
            }
            "response.failed" => {
                return Err(
                    failure(&e["response"]).unwrap_or_else(|| "openai: response failed".into())
                );
            }
            "error" => {
                let msg = e["message"].as_str().unwrap_or_default();
                return Err(if msg.is_empty() {
                    "openai stream error".into()
                } else {
                    format!("openai stream error: {}", yutu_auth::log::quote(msg))
                });
            }
            "response.completed" | "response.incomplete" => terminal = Some(e["response"].clone()),
            _ => {}
        }
    }
    if parts.is_empty()
        && let Some(t) = &terminal
    {
        return convert(t);
    }
    Ok(Content {
        role: Role::Model,
        parts,
    })
}

fn push_text(parts: &mut Vec<Part>, delta: &str, thought: bool) {
    let part = Part::Text {
        text: delta.to_string(),
        thought,
        signature: None,
    };
    merge(parts, part);
}
