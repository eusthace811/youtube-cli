//! Fields and helpers every resource shares, after pkg/common in Go.

use super::OpError;
use serde::Deserialize;
use yutu_api::{Call, Value};

/// Go's `common.Fields`, embedded in every resource input.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Fields {
    pub ids: Vec<String>,
    pub max_results: i64,
    pub hl: String,
    pub channel_id: String,
    pub parts: Vec<String>,
    pub output: String,
    pub on_behalf_of_content_owner: String,
}

/// `common.WithMaxResults`: negative means 1 and zero means everything.
pub fn cli_max_results(n: i64) -> i64 {
    match n {
        n if n < 0 => 1,
        0 => i64::MAX,
        n => n,
    }
}

pub const PER_PAGE: i64 = 20;

/// `common.Paginate`: pages of at most 20, the remaining count dropping by
/// 20 per page whatever the page held, stopping at the last token or an
/// empty page. On error it returns what it fetched plus the joined error.
pub async fn paginate<'s>(
    max_results: i64,
    mut call: impl FnMut() -> Call<'s>,
    sentinel: &str,
) -> (Vec<Value>, Option<OpError>) {
    let mut items = Vec::new();
    let mut remaining = max_results;
    let mut token = String::new();
    while remaining > 0 {
        let mut c = call().param("maxResults", remaining.min(PER_PAGE));
        if !token.is_empty() {
            c = c.param("pageToken", &token);
        }
        let res = match c.send().await {
            Ok(r) => r,
            Err(e) => return (items, Some(OpError::wrap(sentinel, e))),
        };
        let got = res
            .at("items")
            .map(|v| v.as_list().to_vec())
            .unwrap_or_default();
        token = res.str("nextPageToken").to_string();
        remaining = remaining.saturating_sub(PER_PAGE);
        let empty = got.is_empty();
        items.extend(got);
        if token.is_empty() || empty {
            break;
        }
    }
    (items, None)
}

/// Go's `List` tail: print what was fetched, then join the fetch error with
/// any print error. A fetch error with nothing fetched returns early.
pub fn finish_list(
    fetched: (Vec<Value>, Option<OpError>),
    print: impl FnOnce(&[Value]) -> Result<(), String>,
) -> Result<(), OpError> {
    let (items, err) = fetched;
    if let Some(e) = &err
        && items.is_empty()
    {
        return Err(e.clone());
    }
    let mut errs = err.unwrap_or_default();
    if let Err(e) = print(&items) {
        errs.push(e);
    }
    errs.into_result()
}

/// The extensions Go's `pkg.OpenFile` refuses, so secrets never upload.
pub const BLOCKED_EXTENSIONS: &[&str] = &[
    ".json", ".env", ".key", ".pem", ".crt", ".cert", ".pfx", ".p12",
];

/// `pkg.OpenFile` for uploads: blocked extensions are refused and the path
/// must stay under the root.
pub fn open_media(name: &str) -> Result<yutu_api::Media, String> {
    crate::fsroot::open_media(name)
}
