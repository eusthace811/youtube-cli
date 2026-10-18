//! Replays the ADK REST, A2A, trigger and web UI requests recorded from the
//! Go launcher (tests/fixtures/adk-rest.json, written by
//! `parity/adk_rest.py record`) against head and asserts the same status,
//! headers and body, normalized as the recorder normalized Go's.

mod common;

use common::{TOKEN, block_on, http, web};
use serde_json::{Map, Value, json};

const FIXTURE: &str = include_str!("fixtures/adk-rest.json");

/// Responses head changes on purpose, with the divergence that explains
/// each: `/version` names the Rust toolchain, and the A2A card declares the
/// bearer token every route requires (E11).
fn expected(name: &str, mut go: Value) -> Value {
    match name {
        "version" => {
            go["body"]["json"]["language"] = "rust".into();
            go["body"]["json"]["language_version"] = env!("YOUTUBE_CLI_RUSTC_VERSION").into();
        }
        "agent card" => {
            let card = go["body"]["json"].as_object_mut().unwrap();
            card.insert(
                "securitySchemes".into(),
                json!({"bearer": {"type": "http", "scheme": "bearer",
                    "description": "The per-launch token youtube agent prints at startup."}}),
            );
            card.insert("security".into(), json!([{"bearer": []}]));
        }
        _ => {}
    }
    go
}

fn is_uuid_at(b: &[u8], i: usize) -> bool {
    const GROUPS: [usize; 5] = [8, 4, 4, 4, 12];
    let mut j = i;
    for (n, len) in GROUPS.iter().enumerate() {
        for _ in 0..*len {
            if !b
                .get(j)
                .is_some_and(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
            {
                return false;
            }
            j += 1;
        }
        if n < 4 {
            if b.get(j) != Some(&b'-') {
                return false;
            }
            j += 1;
        }
    }
    true
}

/// `\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(\.\d+)?(Z|[+-]\d\d:\d\d)` at `i`: its length.
fn time_at(b: &[u8], i: usize) -> Option<usize> {
    let digit = |k: usize| b.get(i + k).is_some_and(u8::is_ascii_digit);
    let lit = |k: usize, c: u8| b.get(i + k) == Some(&c);
    let shape = [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18]
        .iter()
        .all(|k| digit(*k))
        && lit(4, b'-')
        && lit(7, b'-')
        && lit(10, b'T')
        && lit(13, b':')
        && lit(16, b':');
    if !shape {
        return None;
    }
    let mut k = 19;
    if lit(k, b'.') && digit(k + 1) {
        k += 1;
        while digit(k) {
            k += 1;
        }
    }
    if lit(k, b'Z') {
        return Some(k + 1);
    }
    if (lit(k, b'+') || lit(k, b'-'))
        && digit(k + 1)
        && digit(k + 2)
        && lit(k + 3, b':')
        && digit(k + 4)
        && digit(k + 5)
    {
        return Some(k + 6);
    }
    None
}

fn sha256_hex(s: &[u8]) -> String {
    let d = ring::digest::digest(&ring::digest::SHA256, s);
    d.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

/// parity/adk_rest.py's `scrub`.
fn scrub(v: Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.into_iter()
                .map(|(k, x)| {
                    let x = if k == "lastUpdateTime" && x.is_number() {
                        "<time>".into()
                    } else {
                        x
                    };
                    (k, scrub(x))
                })
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.into_iter().map(scrub).collect()),
        Value::String(s) if s.chars().count() > 400 => {
            format!("<sha256:{}>", &sha256_hex(s.as_bytes())[..16]).into()
        }
        Value::String(s) => {
            let b = s.as_bytes();
            let mut out = String::new();
            let mut i = 0;
            while i < b.len() {
                if is_uuid_at(b, i) {
                    out.push_str("<uuid>");
                    i += 36;
                } else if let Some(n) = time_at(b, i) {
                    out.push_str("<time>");
                    i += n;
                } else {
                    let ch = s[i..].chars().next().unwrap();
                    out.push(ch);
                    i += ch.len_utf8();
                }
            }
            out.into()
        }
        other => other,
    }
}

/// parity/adk_rest.py's `body_of`.
fn body_of(content_type: &str, raw: &[u8]) -> Value {
    let text = String::from_utf8_lossy(raw).into_owned();
    if content_type.contains("text/event-stream") {
        let events: Vec<Value> = text
            .split("\n\n")
            .filter(|c| !c.trim().is_empty())
            .map(|chunk| {
                let mut ev = Map::new();
                for line in chunk.split('\n') {
                    let (k, v) = line.split_once(": ").unwrap_or((line, ""));
                    let v = if k == "data" {
                        serde_json::from_str(v).unwrap()
                    } else {
                        Value::String(v.into())
                    };
                    ev.insert(k.into(), v);
                }
                scrub(Value::Object(ev))
            })
            .collect();
        return json!({ "sse": events });
    }
    if content_type.contains("json")
        && let Ok(v) = serde_json::from_str::<Value>(&text)
    {
        let newline = if text.ends_with('\n') {
            Value::Null
        } else {
            "no-newline".into()
        };
        return json!({"json": scrub(v), "text": newline});
    }
    if raw.len() > 2000 {
        return json!({ "sha256": sha256_hex(raw) });
    }
    json!({ "text": scrub(Value::String(text)) })
}

const KEEP: [&str; 8] = [
    "content-type",
    "allow",
    "location",
    "cache-control",
    "access-control-allow-origin",
    "access-control-allow-methods",
    "access-control-allow-headers",
    "vary",
];

#[test]
fn head_answers_every_recorded_request_as_go_did() {
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    let script = fixture["script"].as_array().unwrap().clone();
    let args = fixture["args"].as_str().unwrap().to_string();
    block_on(async move {
        let w = web(&args, script).await;
        let mut mismatches = Vec::new();
        for ex in fixture["exchanges"].as_array().unwrap() {
            let name = ex["name"].as_str().unwrap();
            let req = &ex["request"];
            let body: Vec<u8> = match &req["body"] {
                Value::Null => Vec::new(),
                Value::String(s) => s.clone().into_bytes(),
                Value::Object(o) => {
                    let parts = o["big"].as_array().unwrap();
                    let (pre, post) = (parts[0].as_str().unwrap(), parts[1].as_str().unwrap());
                    let fill = "x".repeat(10 << 20);
                    let tail = if pre.is_empty() { "x" } else { "" };
                    format!("{pre}{fill}{post}{tail}").into_bytes()
                }
                other => panic!("{other}"),
            };
            let mut headers: Vec<(String, String)> = req["headers"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
                .collect();
            if !body.is_empty()
                && !headers
                    .iter()
                    .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            {
                headers.push(("Content-Type".into(), "application/json".into()));
            }
            headers.push(("Authorization".into(), format!("Bearer {TOKEN}")));
            let refs: Vec<(&str, &str)> = headers
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            let r = http(
                w.port,
                req["method"].as_str().unwrap(),
                req["path"].as_str().unwrap(),
                &refs,
                &body,
            )
            .await;
            let mut kept = Map::new();
            for k in KEEP {
                if let Some(v) = r.header(k) {
                    kept.insert(k.into(), scrub(Value::String(v.into())));
                }
            }
            let got = json!({
                "status": r.status,
                "headers": kept,
                "body": body_of(r.header("content-type").unwrap_or_default(), &r.body),
            });
            let want = expected(
                name,
                json!({"status": ex["status"], "headers": ex["headers"], "body": ex["body"]}),
            );
            if got != want {
                mismatches.push(format!("{name}:\n  go:   {want}\n  head: {got}"));
            }
        }
        assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
        let youtube: Vec<String> = w
            .youtube
            .lock()
            .unwrap()
            .iter()
            .map(|(l, _)| l.clone())
            .collect();
        let recorded: Vec<String> = fixture["youtube"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                let r = r.as_str().unwrap();
                r.split(' ').take(2).collect::<Vec<_>>().join(" ")
            })
            .collect();
        let head: Vec<String> = youtube
            .iter()
            .map(|l| {
                let mut p = l.split(' ');
                let method = p.next().unwrap();
                let path = p.next().unwrap().split('?').next().unwrap();
                format!("{method} {path}")
            })
            .collect();
        assert_eq!(head, recorded, "same YouTube calls as Go");
    });
}
