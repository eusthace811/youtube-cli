//! Fields and helpers every resource shares, after pkg/common in Go.

use super::OpError;
use crate::output::{Cell, Out, table};
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

/// Receives each page of a paginated list as it arrives.
pub trait PageSink: Send {
    fn page(&mut self, items: Vec<Value>);
}

/// Keeps every item, for callers that need the whole list.
#[derive(Default)]
pub struct Collect {
    pub items: Vec<Value>,
}

impl PageSink for Collect {
    fn page(&mut self, items: Vec<Value>) {
        self.items.extend(items);
    }
}

/// `common.Paginate`: pages of at most 20, the remaining count dropping by
/// 20 per page whatever the page held, stopping at the last token or an
/// empty page. Unlike Go it also stops when a page token comes back a
/// second time, since a cycling server would otherwise loop forever once
/// the limit is lifted (divergence E17). Each page goes to `sink`; on error the pages fetched so far
/// have been delivered and the joined error is returned. Returns the number
/// of items fetched.
pub async fn paginate_to<'s>(
    max_results: i64,
    mut call: impl FnMut() -> Call<'s>,
    sentinel: &str,
    sink: &mut dyn PageSink,
) -> (usize, Option<OpError>) {
    let mut count = 0;
    let mut remaining = max_results;
    let mut token = String::new();
    let mut seen = std::collections::HashSet::new();
    while remaining > 0 {
        let mut c = call().param("maxResults", remaining.min(PER_PAGE));
        if !token.is_empty() {
            c = c.param("pageToken", &token);
        }
        let mut res = match c.send().await {
            Ok(r) => r,
            Err(e) => return (count, Some(OpError::wrap(sentinel, e))),
        };
        token = res.str("nextPageToken").to_string();
        let got = take_items(&mut res);
        remaining = remaining.saturating_sub(PER_PAGE);
        let empty = got.is_empty();
        count += got.len();
        sink.page(got);
        if token.is_empty() || empty || !seen.insert(token.clone()) {
            break;
        }
    }
    (count, None)
}

/// Moves the `items` list out of a list response without copying it.
pub fn take_items(res: &mut Value) -> Vec<Value> {
    let Some(s) = res.as_struct_mut() else {
        return Vec::new();
    };
    match s
        .index("items")
        .map(|i| std::mem::replace(&mut s.fields[i], Value::Nil))
    {
        Some(Value::List(items)) => items,
        _ => Vec::new(),
    }
}

/// [`paginate_to`] collecting every item.
pub async fn paginate<'s>(
    max_results: i64,
    call: impl FnMut() -> Call<'s>,
    sentinel: &str,
) -> (Vec<Value>, Option<OpError>) {
    let mut all = Collect::default();
    let (_, err) = paginate_to(max_results, call, sentinel, &mut all).await;
    (all.items, err)
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

/// `PrintList` over a paginated list, encoding each page as it arrives so
/// memory holds one page rather than the whole list. The bytes written equal
/// collecting everything first, as Go does.
pub struct ListSink<'o, 'w, F> {
    out: &'o mut Out<'w>,
    // On a terminal the output is written once at the end, like Go's single
    // write, since a pseudo-terminal can render many small writes
    // differently. Pipes and MCP buffers stream.
    held: Option<String>,
    output: String,
    header: &'static [&'static str],
    row: F,
    rows: Vec<Vec<Cell>>,
    count: usize,
    write_err: Option<String>,
}

impl<'o, 'w, F: Fn(&Value) -> Vec<Cell> + Send> ListSink<'o, 'w, F> {
    pub fn new(
        out: &'o mut Out<'w>,
        output: &str,
        header: &'static [&'static str],
        row: F,
    ) -> Self {
        let held = out.interactive.then(String::new);
        ListSink {
            out,
            held,
            output: output.to_string(),
            header,
            row,
            rows: Vec::new(),
            count: 0,
            write_err: None,
        }
    }

    fn write(&mut self, s: &str) {
        if let Some(held) = &mut self.held {
            held.push_str(s);
            return;
        }
        if self.write_err.is_none()
            && let Err(e) = self.out.write_str(s)
        {
            self.write_err = Some(e);
        }
    }

    /// Ends the list: a fetch error with nothing fetched is returned alone,
    /// otherwise the closing bytes are written and errors join as in Go.
    pub fn finish(mut self, err: Option<OpError>) -> Result<(), OpError> {
        if let Some(e) = &err
            && self.count == 0
        {
            return Err(e.clone());
        }
        match self.output.as_str() {
            "json" => {
                let mut s = String::new();
                yutu_api::json::list_end(self.out.interactive, self.count, &mut s);
                s.push('\n');
                self.write(&s);
            }
            "yaml" => self.write(if self.count == 0 { "[]\n\n" } else { "\n" }),
            "table" => {
                let t = table::render(self.header, &self.rows);
                self.write(&t);
            }
            _ => {}
        }
        if let Some(held) = self.held.take() {
            self.write(&held);
        }
        let mut errs = err.unwrap_or_default();
        if let Some(e) = self.write_err {
            errs.push(e);
        }
        errs.into_result()
    }
}

impl<F: Fn(&Value) -> Vec<Cell> + Send> PageSink for ListSink<'_, '_, F> {
    fn page(&mut self, items: Vec<Value>) {
        for item in items {
            match self.output.as_str() {
                "json" => {
                    let mut s = String::new();
                    yutu_api::json::list_item(&item, self.out.interactive, self.count == 0, &mut s);
                    self.write(&s);
                }
                "yaml" => {
                    let s = yutu_api::yaml::list(std::slice::from_ref(&item));
                    self.write(&s);
                }
                "table" => {
                    let r = (self.row)(&item);
                    self.rows.push(r);
                }
                _ => {}
            }
            self.count += 1;
        }
    }
}

/// `pkg.OpenFile` for uploads: blocked extensions are refused and the path
/// must stay under the root. The opened handle is what gets uploaded, so the
/// file cannot be swapped between the check and the read.
pub fn open_media(name: &str) -> Result<yutu_api::Media, String> {
    let root = crate::fsroot::Root::global()?;
    root.open_read(name)
        .map(yutu_api::Media::Open)
        .map_err(|e| e.to_string())
}

/// A file to write under the root, refusing blocked extensions so a
/// download can never overwrite a token or secret (divergence E1).
pub fn create_write(name: &str) -> Result<std::fs::File, String> {
    let root = crate::fsroot::Root::global()?;
    root.create_write(name).map_err(|e| e.to_string())
}
