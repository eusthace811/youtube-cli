//! ADK's `webui` sublauncher: the embedded dev UI bundle at `/ui/`, its
//! runtime config naming the REST API, and the redirects into it.

use serde_json::json;

use super::http::{Req, Resp};
use crate::launcher::WebUiConfig;

mod bundle {
    include!(concat!(env!("OUT_DIR"), "/webui.rs"));
}

pub const PREFIX: &str = "/ui/";
const INDEX: &str = "index.html";

fn file(name: &str) -> Option<&'static [u8]> {
    bundle::FILES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, b)| *b)
}

fn is_dir(name: &str) -> bool {
    let prefix = format!("{name}/");
    name == "." || bundle::FILES.iter().any(|(n, _)| n.starts_with(&prefix))
}

/// `mime.TypeByExtension` for the bundle's file types.
fn content_type(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, e)| e) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("woff2") => "font/woff2",
        Some("ico") => "image/vnd.microsoft.icon",
        _ => "application/octet-stream",
    }
}

/// index.html with its `<base href>` pointing at the mount, as ADK serves
/// it for deep links.
fn index() -> Vec<u8> {
    let html = String::from_utf8_lossy(file(INDEX).unwrap_or_default());
    html.replacen("<base href=\"./\"", &format!("<base href=\"{PREFIX}\""), 1)
        .into_bytes()
}

/// `path.Clean("/" + rel)` without its leading slash, "." for the root.
fn clean(rel: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    }
}

/// The routes this sublauncher owns, or `None` to fall through.
pub fn handle(cfg: &WebUiConfig, req: &Req) -> Option<Resp> {
    let path = req.path.as_str();
    let get = matches!(req.method.as_str(), "GET" | "HEAD");
    if path == "/" && get {
        return Some(Resp::redirect(302, PREFIX));
    }
    if path == "/ui" && get {
        return Some(Resp::redirect(301, PREFIX));
    }
    let rel = path.strip_prefix(PREFIX)?;
    if !get {
        return Some(Resp::new(405));
    }
    if rel == "assets/config/runtime-config.json" {
        return Some(Resp::json(
            200,
            &json!({ "backendUrl": cfg.api_server_address }),
        ));
    }
    let name = clean(rel);
    let html = |body: Vec<u8>| {
        Resp::new(200)
            .header("accept-ranges", "bytes")
            .header("content-type", content_type(INDEX))
            .body(body)
    };
    if is_dir(&name) {
        if name == "." || file(&format!("{name}/{INDEX}")).is_some() {
            return Some(html(index()));
        }
        return Some(Resp::not_found());
    }
    let Some(bytes) = file(&name) else {
        let has_ext = name.rsplit('/').next().is_some_and(|b| b.contains('.'));
        return Some(if has_ext {
            Resp::not_found()
        } else {
            html(index())
        });
    };
    if name == INDEX {
        // http.FileServer sends /index.html to its directory.
        return Some(Resp::redirect(301, "./"));
    }
    Some(
        Resp::new(200)
            .header("accept-ranges", "bytes")
            .header("content-type", content_type(&name))
            .body(bytes),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_is_embedded_whole() {
        assert_eq!(bundle::FILES.len(), 106);
        assert!(file("index.html").is_some());
        assert!(file("assets/config/runtime-config.json").is_some());
        assert!(
            String::from_utf8(index())
                .unwrap()
                .contains("<base href=\"/ui/\">")
        );
        assert_eq!(clean("a/../b/./c"), "b/c");
        assert_eq!(clean(""), ".");
    }
}
