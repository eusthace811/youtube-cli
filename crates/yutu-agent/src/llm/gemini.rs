//! Gemini's `generateContent` and `streamGenerateContent?alt=sse`, with the
//! request body genai builds for ADK.

use reqwest::header::{CONTENT_TYPE, HeaderValue};
use serde_json::{Map, Value, json};
use yutu_auth::Secret;

use super::{Meta, OnChunk, REQUEST_TIMEOUT, Reply, join_url, sse};
use crate::model::{Content, FunctionCall, Part, Request, Role, merge};

pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/";
const API_VERSION: &str = "v1beta";
/// ADK's prompt when the history does not end with a user turn.
const CONTINUE: &str = "Continue processing previous requests as instructed. Exit or provide a summary if no more outputs are needed.";

pub struct Gemini {
    http: reqwest::Client,
    base_url: String,
    model: String,
    key: Secret,
}

impl Gemini {
    pub fn new(model: &str, key: Secret, base_url: &str) -> Gemini {
        Gemini {
            http: super::http_client(),
            base_url: base_url.to_string(),
            model: model.to_string(),
            key,
        }
    }

    pub async fn generate(
        &self,
        req: &Request<'_>,
        stream: bool,
        on_chunk: OnChunk<'_>,
    ) -> Result<Reply, String> {
        let model = if self.model.starts_with("models/") || self.model.starts_with("tunedModels/") {
            self.model.clone()
        } else {
            format!("models/{}", self.model)
        };
        let path = if stream {
            format!("{API_VERSION}/{model}:streamGenerateContent?alt=sse")
        } else {
            format!("{API_VERSION}/{model}:generateContent")
        };
        let url = join_url(&self.base_url, &path);
        let mut builder = self
            .http
            .post(&url)
            .header(CONTENT_TYPE, "application/json")
            .body(body(req).to_string());
        if !self.key.expose().is_empty() {
            let mut key = HeaderValue::from_bytes(self.key.expose()).map_err(|e| e.to_string())?;
            key.set_sensitive(true);
            builder = builder.header("x-goog-api-key", key);
        }
        if !stream {
            builder = builder.timeout(REQUEST_TIMEOUT);
        }
        let send_err = |e: reqwest::Error| {
            format!(
                "doRequest: error sending request: Post {}: {}",
                yutu_auth::log::quote(&url),
                super::describe(&e)
            )
        };
        if !stream {
            let resp = builder
                .send()
                .await
                .map_err(|e| format!("failed to call model: {}", send_err(e)))?;
            let status = resp.status();
            let text = resp.text().await.map_err(|e| e.to_string())?;
            if !status.is_success() {
                return Err(format!(
                    "failed to call model: {}",
                    api_error(status, &text)
                ));
            }
            let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            let candidates = v["candidates"].as_array().filter(|c| !c.is_empty());
            let Some(first) = candidates.and_then(|c| c.first()) else {
                return Err("empty response".into());
            };
            return Ok(Reply {
                content: content(&first["content"]),
                meta: Meta {
                    turn_complete: false,
                    ..meta(&v)
                },
            });
        }
        let resp = builder.send().await.map_err(send_err)?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(api_error(status, &text));
        }
        let mut events = sse::Reader::new(resp);
        let mut parts: Vec<Part> = Vec::new();
        let mut total = Meta::default();
        while let Some(event) = events.next().await? {
            let chunk: Value = serde_json::from_str(&event.data).map_err(|e| {
                format!(
                    "iterateResponseStream: error unmarshalling data data:{}. error: {e}",
                    event.data
                )
            })?;
            if chunk.get("error").is_some() {
                return Err(api_error(reqwest::StatusCode::OK, &event.data));
            }
            let piece = content(&chunk["candidates"][0]["content"]);
            let chunk_meta = meta(&chunk);
            on_chunk(&piece, &chunk_meta);
            for part in piece.parts {
                merge(&mut parts, part);
            }
            // ADK's aggregator keeps the last usage and finish reason seen;
            // the aggregated reply carries no model version.
            if chunk_meta.usage.is_some() {
                total.usage = chunk_meta.usage;
            }
            if chunk_meta.finish_reason.is_some() {
                total.finish_reason = chunk_meta.finish_reason;
            }
        }
        Ok(Reply {
            content: Content {
                role: Role::Model,
                parts,
            },
            meta: total,
        })
    }
}

/// genai's `APIError` text for a non-2xx answer.
fn api_error(status: reqwest::StatusCode, body: &str) -> String {
    let status_line = format!(
        "{} {}",
        status.as_u16(),
        status.canonical_reason().unwrap_or_default()
    );
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let (code, message, status_text, details) = match parsed.as_ref().and_then(|v| v.get("error")) {
        Some(e) => (
            e["code"].as_i64().unwrap_or(0),
            e["message"].as_str().unwrap_or_default().to_string(),
            e["status"].as_str().unwrap_or_default().to_string(),
            e["details"].as_array().map(|d| {
                d.iter()
                    .map(crate::gojson::marshal)
                    .collect::<Vec<_>>()
                    .join(" ")
            }),
        ),
        None => (
            i64::from(status.as_u16()),
            body.to_string(),
            status_line,
            None,
        ),
    };
    format!(
        "Error {code}, Message: {message}, Status: {status_text}, Details: [{}]",
        details.unwrap_or_default()
    )
}

fn body(req: &Request<'_>) -> Value {
    let mut contents: Vec<Value> = req.contents.iter().map(content_json).collect();
    if req.contents.last().is_some_and(|c| c.role != Role::User) {
        contents.push(content_json(&Content::user_text(CONTINUE)));
    }
    let mut tools: Vec<Value> = Vec::new();
    if req.google_search {
        tools.push(json!({"googleSearch": {}}));
    }
    if !req.declarations.is_empty() {
        let decls: Vec<Value> = req
            .declarations
            .iter()
            .map(|d| {
                let mut m = Map::new();
                m.insert("description".into(), d.description.clone().into());
                m.insert("name".into(), d.name.clone().into());
                m.insert("parametersJsonSchema".into(), d.parameters.clone());
                if let Some(r) = &d.response {
                    m.insert("responseJsonSchema".into(), r.clone());
                }
                Value::Object(m)
            })
            .collect();
        tools.push(json!({"functionDeclarations": decls}));
    }
    let mut body = json!({
        "contents": contents,
        "generationConfig": {},
        "systemInstruction": {"parts": [{"text": req.system}], "role": "user"},
    });
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    body
}

fn content_json(c: &Content) -> Value {
    let parts: Vec<Value> = c
        .parts
        .iter()
        .map(|p| match p {
            Part::Text {
                text,
                thought,
                signature,
            } => {
                let mut m = json!({"text": text});
                if *thought {
                    m["thought"] = Value::Bool(true);
                }
                if let Some(s) = signature {
                    m["thoughtSignature"] = s.clone().into();
                }
                m
            }
            Part::Call { call, signature } => {
                let mut fc = json!({"name": call.name, "args": call.args});
                if let Some(id) = &call.id {
                    fc["id"] = id.clone().into();
                }
                let mut m = json!({"functionCall": fc});
                if let Some(s) = signature {
                    m["thoughtSignature"] = s.clone().into();
                }
                m
            }
            Part::Response(r) => {
                let mut fr = json!({"name": r.name, "response": r.response});
                if let Some(id) = &r.id {
                    fr["id"] = id.clone().into();
                }
                json!({"functionResponse": fr})
            }
        })
        .collect();
    let role = match c.role {
        Role::User => "user",
        Role::Model => "model",
    };
    json!({"parts": parts, "role": role})
}

/// A response's metadata; a chunk with a finish reason completes the turn.
fn meta(v: &Value) -> Meta {
    let finish_reason = v["candidates"][0]["finishReason"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(String::from);
    Meta {
        usage: v.get("usageMetadata").filter(|u| u.is_object()).cloned(),
        turn_complete: finish_reason.is_some(),
        finish_reason,
        model_version: v["modelVersion"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(String::from),
    }
}

fn content(v: &Value) -> Content {
    let parts = v["parts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let signature = p["thoughtSignature"].as_str().map(String::from);
            if let Some(fc) = p.get("functionCall") {
                return Some(Part::Call {
                    call: FunctionCall {
                        id: fc["id"].as_str().map(String::from),
                        name: fc["name"].as_str().unwrap_or_default().to_string(),
                        args: fc["args"].as_object().cloned().unwrap_or_default(),
                    },
                    signature,
                });
            }
            let text = p.get("text")?.as_str()?.to_string();
            Some(Part::Text {
                text,
                thought: p["thought"] == Value::Bool(true),
                signature,
            })
        })
        .collect();
    Content {
        role: Role::Model,
        parts,
    }
}
