//! The 74 tools, derived from the command registry: their `tools/list`
//! entries, argument binding the way go-sdk's `toolForErr` does it, and the
//! op run that fills the text content.

use crate::schema;
use serde_json::{Value, json};
use std::sync::OnceLock;
use yutu_core::ops::{self, Ctx, Entry, Invoke, ServiceProvider};
use yutu_core::output::Out;
use yutu_core::registry::{self, Effect};

/// go-sdk's `ListCacheTTL` as cobra-mcp sets it for yutu.
pub const LIST_TTL_MS: u64 = 7_200_000;

pub struct Tool {
    pub name: &'static str,
    /// The command path the op is registered under, e.g. `video list`.
    pub path: &'static str,
    schema: Value,
    entry: Option<&'static Entry>,
}

struct Catalog {
    /// Sorted by name, the order go-sdk's feature set lists them in.
    tools: Vec<Tool>,
    list: String,
}

fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let mut defs: Vec<(&registry::Command, &registry::Tool)> = registry::tools().collect();
        defs.sort_by_key(|(_, t)| t.name);
        let mut listed = Vec::with_capacity(defs.len());
        let mut tools = Vec::with_capacity(defs.len());
        for (cmd, t) in defs {
            let schema: Value =
                serde_json::from_str(t.input_schema).expect("registry input schema is JSON");
            listed.push(json!({
                "annotations": annotations(t),
                "description": t.description,
                "inputSchema": schema,
                "name": t.name,
                "title": t.title,
            }));
            tools.push(Tool {
                name: t.name,
                path: cmd.path,
                schema,
                entry: ops::find(cmd.path),
            });
        }
        let list = json!({"ttlMs": LIST_TTL_MS, "cacheScope": "public", "tools": listed});
        Catalog {
            tools,
            list: list.to_string(),
        }
    })
}

/// The four hints exactly as the Go command files set them.
fn annotations(t: &registry::Tool) -> Value {
    json!({
        "destructiveHint": t.effect == Effect::Destructive,
        "idempotentHint": t.idempotent,
        "openWorldHint": true,
        "readOnlyHint": t.effect == Effect::Read,
    })
}

/// The `tools/list` result, serialized once.
pub fn list_result() -> &'static str {
    &catalog().list
}

pub fn find(name: &str) -> Option<&'static Tool> {
    let tools = &catalog().tools;
    tools
        .binary_search_by(|t| t.name.cmp(name))
        .ok()
        .map(|i| &tools[i])
}

pub fn count() -> usize {
    catalog().tools.len()
}

pub struct Bound {
    pub invoke: Box<dyn Invoke>,
    /// The arguments after defaults, as the op received them.
    pub input: Value,
}

impl Tool {
    /// Validates the arguments, applies schema defaults and decodes them into
    /// the op. `arguments` is the JSON text the client sent. The error is the
    /// text of go-sdk's `isError` result.
    pub fn bind(&self, arguments: Option<&str>) -> Result<Bound, String> {
        let input = schema::apply(&self.schema, arguments)
            .map_err(|e| format!("validating \"arguments\": {e}"))?;
        let entry = self
            .entry
            .ok_or_else(|| format!("{} is not available in this build", self.name))?;
        let invoke = (entry.mcp)(input.clone())?;
        Ok(Bound { invoke, input })
    }
}

/// Runs a bound op into a buffer: the buffer is the text content on success,
/// the op's error text otherwise.
pub async fn run(invoke: &dyn Invoke, provider: &dyn ServiceProvider) -> Result<String, String> {
    let mut buf: Vec<u8> = Vec::new();
    let res = {
        let mut ctx = Ctx::new(
            Out {
                w: &mut buf,
                interactive: false,
            },
            provider,
        );
        invoke.run(&mut ctx).await
    };
    res.map(|()| String::from_utf8_lossy(&buf).into_owned())
        .map_err(|e| e.to_string())
}

pub fn text_result(text: &str, is_error: bool) -> Value {
    let mut r = json!({"content": [{"type": "text", "text": text}]});
    if is_error {
        r["isError"] = Value::Bool(true);
    }
    r
}
