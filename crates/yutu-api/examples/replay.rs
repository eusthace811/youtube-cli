//! Replays recorded Go requests through the generated builders and prints
//! the request each builder produces, for parity/diff.py to compare.
//!
//!     cargo run -p yutu-api --example replay -- case.json... > replayed.jsonl

use serde_json::{Value as Json, json};
use yutu_api::generated::METHODS;
use yutu_api::schema::Location;
use yutu_api::{Service, Value, decode};

fn route(method: &str, path: &str) -> Option<&'static yutu_api::schema::Method> {
    let path = path.trim_start_matches('/');
    METHODS.iter().find(|m| {
        let candidates = [Some(m.path), m.upload_path];
        m.http_method == method
            && candidates.iter().flatten().any(|p| {
                let (a, b): (Vec<_>, Vec<_>) = (p.split('/').collect(), path.split('/').collect());
                a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| x == y || x.starts_with('{'))
            })
    })
}

fn main() {
    let svc = Service::with_base("http://recorder/", None);
    for file in std::env::args().skip(1) {
        let case: Json = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        for req in case["requests"].as_array().unwrap() {
            let (verb, path) = (
                req["method"].as_str().unwrap(),
                req["path"].as_str().unwrap(),
            );
            let Some(m) = route(verb, path) else {
                println!(
                    "{}",
                    json!({"file": file, "error": format!("no method for {verb} {path}")})
                );
                continue;
            };
            let mut call = svc.call(m.id);
            let tmpl = if path.contains("/upload/") {
                m.upload_path.unwrap()
            } else {
                m.path
            };
            for (t, seg) in tmpl.split('/').zip(path.trim_start_matches('/').split('/')) {
                if let Some(name) = t.strip_prefix('{').and_then(|n| n.strip_suffix('}')) {
                    call = call.param(name, seg);
                }
            }
            for (k, vals) in req["query"].as_object().unwrap() {
                if matches!(k.as_str(), "alt" | "prettyPrint" | "uploadType") {
                    continue;
                }
                let vals: Vec<String> = vals
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap().to_string())
                    .collect();
                if m.params
                    .iter()
                    .any(|p| p.name == k && p.location == Location::Path)
                {
                    continue;
                }
                call = if vals.len() == 1 {
                    call.param(k, &vals[0])
                } else {
                    call.params(k, &vals)
                };
            }
            let ctype = req["ctype"].as_str().unwrap();
            let mut body_json = None;
            if ctype == "application/json" {
                body_json =
                    Some(serde_json::from_str::<Json>(req["body"].as_str().unwrap()).unwrap());
            } else if ctype.starts_with("multipart/") {
                let body = req["body"].as_str().unwrap();
                let start = body.find("\r\n\r\n").unwrap() + 4;
                let end = body[start..].find("\r\n--").unwrap() + start;
                if end > start {
                    body_json = Some(serde_json::from_str::<Json>(&body[start..end]).unwrap());
                }
                call = call.media(yutu_api::Media::Bytes(Vec::new()));
            }
            if let (Some(b), Some(ty)) = (&body_json, m.request) {
                let mut v = decode::from_json(ty, b).unwrap();
                force_zero_fields(&mut v, b);
                call = call.body(&v);
            }
            let download = req["query"]["alt"][0] == "media";
            let (verb, url) = if download { call.preview_download() } else { call.preview() };
            let (p, q) = url
                .trim_start_matches("http://recorder")
                .split_once('?')
                .unwrap();
            let mut query = serde_json::Map::new();
            for kv in q.split('&') {
                let (k, v) = kv.split_once('=').unwrap();
                let v = decode_query(v);
                query
                    .entry(k.to_string())
                    .or_insert_with(|| json!([]))
                    .as_array_mut()
                    .unwrap()
                    .push(json!(v));
            }
            println!(
                "{}",
                json!({"file": file, "method": verb, "path": p, "query": query,
                       "ctype": if body_json.is_some() || ctype.starts_with("multipart/") { ctype } else { "" },
                       "body": call.body_text().map(|b| serde_json::from_str::<Json>(b).unwrap())})
            );
        }
    }
}

/// Go sends zero values listed in ForceSendFields; re-mark every zero value
/// present in the recorded body so the re-encoded body keeps it.
fn force_zero_fields(v: &mut Value, j: &Json) {
    let (Some(s), Some(obj)) = (v.as_struct_mut(), j.as_object()) else {
        return;
    };
    let def = yutu_api::schema::type_def(s.ty);
    let mut force = Vec::new();
    for (i, f) in def.fields.iter().enumerate() {
        let Some(jv) = obj.get(f.json) else { continue };
        if f.skip {
            continue;
        }
        let zero = matches!(jv, Json::Bool(false))
            || jv == &json!(0)
            || jv == &json!("")
            || jv == &json!("0")
            || jv == &json!([]);
        if zero {
            force.push(f.go);
        }
        force_zero_fields(&mut s.fields[i], jv);
    }
    for f in force {
        s.force_send(f);
    }
}

fn decode_query(v: &str) -> String {
    let b = v.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' => {
                out.push(u8::from_str_radix(&v[i + 1..i + 3], 16).unwrap());
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8(out).unwrap()
}
