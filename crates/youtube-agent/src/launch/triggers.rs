//! ADK's Pub/Sub push and Eventarc trigger endpoints: each delivery runs
//! the agent once in a fresh session, retried with backoff while the model
//! answers 429. Nobody is there to approve a write, so the agent's writes
//! are declined (divergence E9).

use serde_json::{Map, Value, json};
use std::sync::Arc;

use super::http::{Req, Resp};
use super::session::Input;
use super::web::App;
use super::wire;
use crate::agent;
use crate::launcher::TriggerConfig;

fn respond(status: u16, msg: &str) -> Resp {
    Resp::json(status, &json!({ "status": msg }))
}

/// A JSON kind as Go's unmarshal errors name it.
fn kind(v: &Value) -> &'static str {
    match v {
        Value::Array(_) => "array",
        Value::Object(_) => "object",
        Value::String(_) => "string",
        Value::Number(_) => "number",
        Value::Bool(_) => "bool",
        Value::Null => "null",
    }
}

/// Go's `encoding/base64.StdEncoding.DecodeString`, which `[]byte` fields
/// use, with its error text.
fn base64(s: &str) -> Result<Vec<u8>, String> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let b: Vec<u8> = s.bytes().filter(|c| *c != b'\r' && *c != b'\n').collect();
    let bad = |i: usize| format!("illegal base64 data at input byte {i}");
    if !b.len().is_multiple_of(4) {
        let valid = b.iter().take_while(|c| val(**c).is_some()).count();
        return Err(bad(valid.min(b.len() / 4 * 4)));
    }
    let mut out = Vec::with_capacity(b.len() / 4 * 3);
    for (q, chunk) in b.chunks(4).enumerate() {
        let mut n = 0u32;
        let mut pad = 0;
        for (j, &c) in chunk.iter().enumerate() {
            let v = if c == b'=' && j >= 2 && q == b.len() / 4 - 1 {
                pad += 1;
                0
            } else if pad > 0 {
                return Err(bad(q * 4 + j));
            } else {
                val(c).ok_or_else(|| bad(q * 4 + j))?
            };
            n = (n << 6) | u32::from(v);
        }
        let bytes = n.to_be_bytes();
        out.extend_from_slice(&bytes[1..4 - pad]);
    }
    Ok(out)
}

/// ADK's `messageContentFromPubSub` over a decoded `PubSubTriggerRequest`.
fn pubsub_message(v: &Value, root: &str) -> Result<Result<String, String>, String> {
    let field = |path: &str, want: &str, got: &Value| {
        format!(
            "json: cannot unmarshal {} into Go struct field {root}.{path} of type {want}",
            kind(got)
        )
    };
    let Value::Object(_) = v else {
        if v.is_null() {
            return Ok(Err("empty message data and attributes".into()));
        }
        return Err(format!(
            "json: cannot unmarshal {} into Go value of type models.{root}",
            kind(v)
        ));
    };
    let msg = &v["message"];
    if !matches!(msg, Value::Object(_) | Value::Null) {
        return Err(field("message", "models.PubSubMessage", msg));
    }
    let mut content = Map::new();
    match &msg["data"] {
        Value::Null => {}
        Value::String(s) => {
            let bytes = base64(s)
                .map_err(|e| format!("{}: {e}", field("message.data", "[]uint8", &msg["data"])))?;
            if !bytes.is_empty() {
                content.insert(
                    "data".into(),
                    String::from_utf8_lossy(&bytes).into_owned().into(),
                );
            }
        }
        other => return Err(field("message.data", "[]uint8", other)),
    }
    match &msg["attributes"] {
        Value::Null => {}
        Value::Object(a) => {
            for (k, v) in a {
                if !v.is_string() {
                    return Err(field(&format!("message.attributes.{k}"), "string", v));
                }
            }
            if !a.is_empty() {
                content.insert(
                    "attributes".into(),
                    crate::gojson::sorted(&msg["attributes"]),
                );
            }
        }
        other => return Err(field("message.attributes", "map[string]string", other)),
    }
    if content.is_empty() {
        return Ok(Err("empty message data and attributes".into()));
    }
    Ok(Ok(crate::gojson::marshal(&Value::Object(content))))
}

pub async fn pubsub(app: &Arc<App>, cfg: &TriggerConfig, req: &Req, app_name: &str) -> Resp {
    let decoded = req.body().map_err(String::from).and_then(wire::decode);
    let v = match decoded {
        Ok(v) => v,
        Err(e) => return respond(400, &format!("failed to decode request: {e}")),
    };
    let message = match pubsub_message(&v, "PubSubTriggerRequest") {
        Err(e) => return respond(400, &format!("failed to decode request: {e}")),
        Ok(Err(e)) => return respond(400, &format!("failed to retrieve message content: {e}")),
        Ok(Ok(m)) => m,
    };
    let user = v["subscription"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or("pubsub-caller");
    deliver(app, cfg, app_name, user, message).await
}

pub async fn eventarc(app: &Arc<App>, cfg: &TriggerConfig, req: &Req, app_name: &str) -> Resp {
    let structured = req.header("content-type") == Some("application/cloudevents+json");
    let (event, data): (Map<String, Value>, Option<String>) = if structured {
        let v = match req.body().map_err(String::from).and_then(wire::decode) {
            Ok(Value::Object(m)) => m,
            Ok(Value::Null) => Map::new(),
            Ok(other) => {
                return respond(
                    400,
                    &format!(
                        "failed to unmarshal eventarc request: json: cannot unmarshal {} into Go value of type models.EventarcTriggerRequest",
                        kind(&other)
                    ),
                );
            }
            Err(e) => return respond(400, &format!("failed to unmarshal eventarc request: {e}")),
        };
        let data = v
            .get("data")
            .filter(|d| !d.is_null())
            .map(crate::gojson::encode);
        (v, data)
    } else {
        let body = match req.body() {
            Ok(b) => b,
            Err(e) => return respond(400, &format!("failed to read body: {e}")),
        };
        let mut m = Map::new();
        for (k, h) in [
            ("id", "ce-id"),
            ("type", "ce-type"),
            ("source", "ce-source"),
            ("specversion", "ce-specversion"),
            ("time", "ce-time"),
        ] {
            m.insert(k.into(), req.header(h).unwrap_or_default().into());
        }
        let data = (!body.is_empty()).then(|| String::from_utf8_lossy(body).into_owned());
        (m, data)
    };
    let text = |k: &str| {
        event
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let message = if text("type") == "google.cloud.pubsub.topic.v1.messagePublished" {
        let inner = match data.as_deref().map(|d| wire::decode(d.as_bytes())) {
            None => Ok(Value::Null),
            Some(r) => r,
        };
        let inner = match inner {
            Ok(v) => v,
            Err(e) => return respond(500, &format!("failed to unmarshal pubsub data: {e}")),
        };
        match pubsub_message(&inner, "PubSubTriggerRequest") {
            Err(e) => return respond(500, &format!("failed to unmarshal pubsub data: {e}")),
            Ok(Err(e)) => return respond(400, &format!("failed to retrieve message content: {e}")),
            Ok(Ok(m)) => m,
        }
    } else {
        let mut out = format!(
            "{{\"id\":{},\"source\":{},\"type\":{},\"specversion\":{}",
            crate::gojson::encode(&text("id").into()),
            crate::gojson::encode(&text("source").into()),
            crate::gojson::encode(&text("type").into()),
            crate::gojson::encode(&text("specversion").into()),
        );
        if !text("time").is_empty() {
            out.push_str(&format!(
                ",\"time\":{}",
                crate::gojson::encode(&text("time").into())
            ));
        }
        if let Some(d) = &data {
            // json.RawMessage marshals compacted and HTML-escaped.
            match wire::decode(d.as_bytes()) {
                Ok(v) => out.push_str(&format!(",\"data\":{}", crate::gojson::encode(&v))),
                Err(e) => {
                    return respond(
                        500,
                        &format!(
                            "failed to marshal agent message: json: error calling MarshalJSON for type json.RawMessage: {e}"
                        ),
                    );
                }
            }
        }
        out.push('}');
        out
    };
    let source = text("source");
    let user = if source.is_empty() {
        "eventarc-caller"
    } else {
        source.as_str()
    };
    let _permit = match &app.eventarc_slots {
        Some(s) => s.acquire().await.ok(),
        None => None,
    };
    deliver(app, cfg, app_name, user, message).await
}

/// ADK's `RetriableRunner.RunAgent`: a new session, one user message, and
/// retries with jittered exponential backoff while the model is throttled.
async fn deliver(
    app: &Arc<App>,
    cfg: &TriggerConfig,
    app_name: &str,
    user: &str,
    message: String,
) -> Resp {
    let fail = |e: String| respond(500, &format!("failed to run agent: {e}"));
    let session = match app.store.create(app_name, user, None, Map::new()) {
        Ok(s) => s,
        Err(e) => return fail(format!("failed to create session: {e}")),
    };
    if app_name != agent::NAME {
        return fail(format!(
            "failed to load agent: cannot load agent '{app_name}' - provide an empty string or use '{}'",
            agent::NAME
        ));
    }
    let mut attempt = 0;
    loop {
        let input = Input {
            message: json!({"role": "user", "parts": [{"text": message}]}),
            state_delta: None,
            stream: false,
            human: false,
        };
        match session.run(&app.agent, input, &mut |_| {}).await {
            Ok(()) => return respond(200, "success"),
            Err(e) => {
                let throttled = e.contains("429") || e.contains("ResourceExhausted");
                if !throttled || attempt >= cfg.max_retries {
                    return fail(e);
                }
                let base = cfg.base_delay.as_secs_f64() * 2f64.powi(attempt as i32);
                let delay = base.min(cfg.max_delay.as_secs_f64());
                let jitter =
                    f64::from(u32::from_le_bytes(random4())) / f64::from(u32::MAX) * delay * 0.5;
                tokio::time::sleep(std::time::Duration::from_secs_f64(delay + jitter)).await;
                attempt += 1;
            }
        }
    }
}

fn random4() -> [u8; 4] {
    let mut b = [0u8; 4];
    getrandom::fill(&mut b).expect("OS randomness");
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_go() {
        assert_eq!(base64("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64("aGk=").unwrap(), b"hi");
        assert_eq!(base64("").unwrap(), b"");
        assert_eq!(
            base64("!!!").unwrap_err(),
            "illegal base64 data at input byte 0"
        );
    }
}
