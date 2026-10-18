//! `i18n://hl` and the three resource templates, registered as the Go
//! cmd/i18nLanguage, cmd/i18nRegion and cmd/videoCategory list files do, and
//! read by running the matching list op with JSON output.

use crate::tools::{self, LIST_TTL_MS};
use serde_json::{Value, json};
use yutu_core::ops::{self, ServiceProvider};

const MIME: &str = "application/json";

/// Where the `{hl}` of a read goes.
#[derive(Clone, Copy)]
enum Hl {
    None,
    FromUri,
}

struct Resource {
    /// The URI for a static resource, the URI template otherwise.
    uri: &'static str,
    template: bool,
    name: &'static str,
    title: Option<&'static str>,
    description: &'static str,
    op: &'static str,
    hl: Hl,
    /// `region_code`, which Go hard-codes for video categories.
    region_code: Option<&'static str>,
}

/// Static resources first, then templates sorted by URI template: the order
/// go-sdk lists and matches them in.
const RESOURCES: &[Resource] = &[
    Resource {
        uri: "i18n://hl",
        template: false,
        name: "i18nHostLanguages",
        title: None,
        description: "List all i18n host languages for YouTube regions",
        op: "i18nLanguage list",
        hl: Hl::None,
        region_code: None,
    },
    Resource {
        uri: "i18n://language/{hl}",
        template: true,
        name: "i18nLanguages",
        title: None,
        description: "List i18n languages. Use this tool to list available internationalization languages.",
        op: "i18nLanguage list",
        hl: Hl::FromUri,
        region_code: None,
    },
    Resource {
        uri: "i18n://region/{hl}",
        template: true,
        name: "i18nRegions",
        title: None,
        description: "List i18n regions. Use this tool to list available internationalization regions.",
        op: "i18nRegion list",
        hl: Hl::FromUri,
        region_code: None,
    },
    Resource {
        uri: "video://category/{hl}",
        template: true,
        name: "video categories",
        title: Some("Manage YouTube video categories"),
        description: "Manage YouTube video categories. Use this tool to list available video categories.",
        op: "videoCategory list",
        // Go reads `{hl}` only from i18n URIs, so it ignores it here (divergence E6).
        hl: Hl::FromUri,
        region_code: Some("US"),
    },
];

fn listing(r: &Resource) -> Value {
    let mut v = json!({
        "annotations": {"audience": ["user", "assistant"], "priority": 0.51},
        "description": r.description,
        "mimeType": MIME,
        "name": r.name,
    });
    v[if r.template { "uriTemplate" } else { "uri" }] = Value::String(r.uri.into());
    if let Some(t) = r.title {
        v["title"] = Value::String(t.into());
    }
    v
}

pub fn list_result() -> Value {
    let rs: Vec<Value> = RESOURCES
        .iter()
        .filter(|r| !r.template)
        .map(listing)
        .collect();
    json!({"ttlMs": LIST_TTL_MS, "cacheScope": "public", "resources": rs})
}

pub fn templates_result() -> Value {
    let rs: Vec<Value> = RESOURCES
        .iter()
        .filter(|r| r.template)
        .map(listing)
        .collect();
    json!({"ttlMs": LIST_TTL_MS, "cacheScope": "public", "resourceTemplates": rs})
}

/// A read that failed: either no resource matches, or the op failed.
pub enum ReadError {
    NotFound,
    Op(String),
}

/// The resource matching `uri` and the `{hl}` it carries.
fn lookup(uri: &str) -> Option<(&'static Resource, &str)> {
    if let Some(r) = RESOURCES.iter().find(|r| !r.template && r.uri == uri) {
        return Some((r, ""));
    }
    RESOURCES.iter().filter(|r| r.template).find_map(|r| {
        let prefix = r.uri.strip_suffix("{hl}")?;
        let hl = uri.strip_prefix(prefix)?;
        simple_expansion(hl).then_some((r, hl))
    })
}

/// Whether `s` matches the regexp yosida95/uritemplate builds for a `{var}`
/// simple expansion: unreserved characters, commas and percent escapes.
fn simple_expansion(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b',' => i += 1,
            b'%' if b.get(i + 1).is_some_and(u8::is_ascii_hexdigit)
                && b.get(i + 2).is_some_and(u8::is_ascii_hexdigit) =>
            {
                i += 3
            }
            _ => return false,
        }
    }
    true
}

/// Runs the resource's list op and returns its `resources/read` result.
pub async fn read(
    uri: &str,
    provider: &dyn ServiceProvider,
) -> Result<(Value, &'static str), ReadError> {
    let (r, hl) = lookup(uri).ok_or(ReadError::NotFound)?;
    let mut args = json!({"parts": ["id", "snippet"], "output": "json"});
    if let Hl::FromUri = r.hl
        && !hl.is_empty()
    {
        args["hl"] = Value::String(hl.into());
    }
    if let Some(rc) = r.region_code {
        args["region_code"] = Value::String(rc.into());
    }
    let entry = ops::find(r.op)
        .ok_or_else(|| ReadError::Op(format!("{} is not available in this build", r.op)))?;
    let invoke = (entry.mcp)(args).map_err(ReadError::Op)?;
    let text = tools::run(invoke.as_ref(), provider)
        .await
        .map_err(ReadError::Op)?;
    let result = json!({
        "ttlMs": 0,
        "cacheScope": "public",
        "contents": [{"uri": uri, "mimeType": MIME, "text": text}],
    });
    Ok((result, r.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_templates_like_go() {
        assert_eq!(
            lookup("i18n://hl").map(|(r, _)| r.name),
            Some("i18nHostLanguages")
        );
        assert_eq!(lookup("i18n://language/ja").map(|(_, h)| h), Some("ja"));
        assert_eq!(lookup("i18n://language/").map(|(_, h)| h), Some(""));
        assert_eq!(
            lookup("video://category/pt-BR").map(|(_, h)| h),
            Some("pt-BR")
        );
        assert_eq!(lookup("i18n://region/%41").map(|(_, h)| h), Some("%41"));
        assert!(lookup("i18n://region/a/b").is_none());
        assert!(lookup("i18n://region/%4").is_none());
        assert!(lookup("i18n://nope").is_none());
    }
}
