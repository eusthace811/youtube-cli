//! One-time import of spec/registry/*.toml from the Go command tree dump and
//! the MCP contract. After the import the TOML files are owned by hand.

use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs;

pub fn run(_args: &[String]) -> Result<(), String> {
    let root = crate::repo_root();
    let dump: Value = read_json(&root.join("parity/golden/go-dump.json"))?;
    let contract: Value = read_json(&root.join("parity/golden/mcp-contract.json"))?;
    let tools: BTreeMap<String, Value> = contract["tools"]["result"]["tools"]
        .as_array()
        .ok_or("contract has no tools")?
        .iter()
        .map(|t| (t["name"].as_str().unwrap().to_string(), t.clone()))
        .collect();

    let mut files: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut visit = vec![(dump["commands"].clone(), Vec::<String>::new())];
    while let Some((node, parent)) = visit.pop() {
        let name = node["use"].as_str().unwrap().split_whitespace().next().unwrap().to_string();
        let mut path = parent.clone();
        if !(parent.is_empty() && name == "yutu") {
            path.push(name);
        }
        let file = match path.first().map(String::as_str) {
            None | Some("auth" | "mcp" | "agent" | "version" | "completion" | "help") => "core".to_string(),
            Some(resource) => resource.to_string(),
        };
        files.entry(file).or_default().push(command(&node, &path, &tools));
        if let Some(children) = node["children"].as_array() {
            for c in children.iter().rev() {
                visit.push((c.clone(), path.clone()));
            }
        }
    }

    let dir = root.join("spec/registry");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for (file, commands) in &files {
        let mut doc = Map::new();
        doc.insert("command".into(), Value::Array(commands.clone()));
        let text = toml::to_string_pretty(&doc).map_err(|e| e.to_string())?;
        fs::write(dir.join(format!("{file}.toml")), text).map_err(|e| e.to_string())?;
    }
    eprintln!(
        "registry: {} files, {} commands, {} tools",
        files.len(),
        files.values().map(Vec::len).sum::<usize>(),
        tools.len()
    );
    Ok(())
}

fn command(node: &Value, path: &[String], tools: &BTreeMap<String, Value>) -> Value {
    let mut c = Map::new();
    c.insert("path".into(), Value::String(path.join(" ")));
    for key in ["use", "short", "long", "example"] {
        if let Some(s) = node[key].as_str().filter(|s| !s.is_empty()) {
            c.insert(key.into(), Value::String(s.into()));
        }
    }
    if let Some(a) = node["aliases"].as_array().filter(|a| !a.is_empty()) {
        c.insert("aliases".into(), Value::Array(a.clone()));
    }
    if node["hidden"].as_bool() == Some(true) {
        c.insert("hidden".into(), Value::Bool(true));
    }
    c.insert("runnable".into(), node["runnable"].clone());
    if path.len() == 2 {
        let tool_name = format!("{}-{}", path[0], path[1]);
        if let Some(t) = tools.get(&tool_name) {
            let a = &t["annotations"];
            let effect = if path[0] == "caption" && path[1] == "download" {
                "writes_local_file"
            } else if a["readOnlyHint"] == Value::Bool(true) {
                "read"
            } else if a["destructiveHint"] == Value::Bool(true) {
                "destructive"
            } else {
                "write"
            };
            let mut tool = Map::new();
            tool.insert("name".into(), Value::String(tool_name));
            tool.insert("title".into(), t["title"].clone());
            tool.insert("description".into(), t["description"].clone());
            tool.insert("effect".into(), Value::String(effect.into()));
            tool.insert("idempotent".into(), a["idempotentHint"].clone());
            tool.insert(
                "input_schema".into(),
                Value::String(serde_json::to_string_pretty(&t["inputSchema"]).unwrap()),
            );
            c.insert("tool".into(), Value::Object(tool));
        }
    }
    let flags: Vec<Value> = node["flags"]
        .as_array()
        .map(|fs| fs.iter().map(flag).collect())
        .unwrap_or_default();
    if !flags.is_empty() {
        c.insert("flag".into(), Value::Array(flags));
    }
    Value::Object(c)
}

fn flag(f: &Value) -> Value {
    let mut out = Map::new();
    out.insert("name".into(), f["name"].clone());
    if let Some(s) = f["shorthand"].as_str().filter(|s| !s.is_empty()) {
        out.insert("short".into(), Value::String(s.into()));
    }
    out.insert("type".into(), f["type"].clone());
    out.insert("default".into(), f["default"].clone());
    if let Some(s) = f["noOptDefVal"].as_str().filter(|s| !s.is_empty()) {
        out.insert("no_opt_default".into(), Value::String(s.into()));
    }
    out.insert("usage".into(), f["usage"].clone());
    if f["hidden"].as_bool() == Some(true) {
        out.insert("hidden".into(), Value::Bool(true));
    }
    if f["persistent"].as_bool() == Some(true) {
        out.insert("persistent".into(), Value::Bool(true));
    }
    if f["annotations"]
        .get("cobra_annotation_bash_completion_one_required_flag")
        .is_some()
    {
        out.insert("required".into(), Value::Bool(true));
    }
    Value::Object(out)
}

fn read_json(path: &std::path::Path) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}
