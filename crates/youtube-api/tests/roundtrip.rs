//! Decodes every parity fixture and re-encodes it, comparing against the bytes
//! Go produced for the same fixture (parity/golden/encode, written by
//! ref/yutu-go-encode).

use std::path::{Path, PathBuf};
use youtube_api::generated::METHODS;
use youtube_api::{Value, decode, json, yaml};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture_key(method: &str, path: &str) -> String {
    let path = path
        .trim_start_matches('/')
        .replacen("upload/youtube/v3/", "upload/", 1)
        .replacen("youtube/v3/", "", 1);
    let path = if path.starts_with("captions/") {
        "captions/_".to_string()
    } else {
        path
    };
    format!("{method}_{}", path.replace('/', "."))
}

#[test]
fn every_fixture_reencodes_like_go() {
    let mut checked = 0;
    let mut failures = Vec::new();
    for m in METHODS {
        let Some(resp) = m.response else { continue };
        for path in [Some(m.path), m.upload_path].into_iter().flatten() {
            let key = fixture_key(m.http_method, path);
            let fx = repo().join("parity/fixtures").join(format!("{key}.json"));
            let Ok(text) = std::fs::read_to_string(&fx) else {
                continue;
            };
            let raw: serde_json::Value = serde_json::from_str(&text).unwrap();
            let pages = match raw.get("pages") {
                Some(p) => p.as_array().unwrap().clone(),
                None => vec![raw.clone()],
            };
            for (i, page) in pages.iter().enumerate() {
                let v = decode::from_json(resp, page).unwrap_or_else(|e| panic!("{key} p{i}: {e}"));
                let base = repo()
                    .join("parity/golden/encode")
                    .join(format!("{key}.p{i}"));
                let mut check = |suffix: &str, got: String| {
                    let want =
                        std::fs::read_to_string(format!("{}{suffix}", base.display())).unwrap();
                    checked += 1;
                    if want != got {
                        let at = want
                            .bytes()
                            .zip(got.bytes())
                            .position(|(a, b)| a != b)
                            .unwrap_or(want.len().min(got.len()));
                        let lo = at.saturating_sub(60);
                        failures.push(format!(
                            "{key}.p{i}{suffix} at byte {at}:\n  go:   {:?}\n  rust: {:?}",
                            want.get(lo..(at + 60).min(want.len())).unwrap_or(""),
                            got.get(lo..(at + 60).min(got.len())).unwrap_or("")
                        ));
                    }
                };
                check(".json", json::value(&v, false));
                check(".indent.json", json::value(&v, true));
                check(".yaml", yaml::value(&v));
                if let Some(items) = v.as_struct().and_then(|s| s.get("items")) {
                    let items: &[Value] = items.as_list();
                    check(".items.json", json::list(items, false));
                    check(".items.indent.json", json::list(items, true));
                    check(".items.yaml", yaml::list(items));
                    let streamed_yaml: String = items
                        .iter()
                        .map(|i| yaml::list(std::slice::from_ref(i)))
                        .collect();
                    check(
                        ".items.yaml",
                        if items.is_empty() {
                            "[]\n".to_string()
                        } else {
                            streamed_yaml
                        },
                    );
                    for indent in [false, true] {
                        let mut streamed = String::new();
                        for (n, i) in items.iter().enumerate() {
                            json::list_item(i, indent, n == 0, &mut streamed);
                        }
                        json::list_end(indent, items.len(), &mut streamed);
                        check(
                            if indent {
                                ".items.indent.json"
                            } else {
                                ".items.json"
                            },
                            streamed,
                        );
                    }
                }
            }
        }
    }
    assert!(checked >= 600, "only {checked} encodings checked");
    assert!(
        failures.is_empty(),
        "{} of {checked} encodings differ:\n{}",
        failures.len(),
        failures[..failures.len().min(15)].join("\n")
    );
}
