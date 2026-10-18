//! Generates the static command registry from spec/registry/*.toml.

use serde::Deserialize;
use std::fmt::Write;
use std::path::Path;

#[derive(Deserialize)]
struct File {
    command: Vec<Command>,
}

#[derive(Deserialize)]
struct Command {
    path: String,
    #[serde(rename = "use")]
    use_line: String,
    #[serde(default)]
    short: String,
    #[serde(default)]
    long: String,
    #[serde(default)]
    example: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    hidden: bool,
    runnable: bool,
    #[serde(default)]
    flag: Vec<Flag>,
    tool: Option<Tool>,
}

#[derive(Deserialize)]
struct Flag {
    name: String,
    #[serde(default)]
    short: String,
    #[serde(rename = "type")]
    kind: String,
    default: String,
    #[serde(default)]
    no_opt_default: String,
    usage: String,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    persistent: bool,
    #[serde(default)]
    required: bool,
}

#[derive(Deserialize)]
struct Tool {
    name: String,
    title: String,
    description: String,
    effect: String,
    idempotent: bool,
    input_schema: String,
}

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/registry");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut commands = Vec::new();
    let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
    names.sort();
    for p in names {
        println!("cargo:rerun-if-changed={}", p.display());
        let f: File = toml::from_str(&std::fs::read_to_string(&p).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        commands.extend(f.command);
    }
    commands.sort_by(|a, b| a.path.cmp(&b.path));
    let index = |path: &str| commands.iter().position(|c| c.path == path).unwrap();

    let mut out = String::new();
    writeln!(out, "pub static COMMANDS: &[Command] = &[").unwrap();
    for c in &commands {
        let parent = c.path.rsplit_once(' ').map(|(p, _)| p).unwrap_or("");
        let parent = if c.path.is_empty() { "None".to_string() } else { format!("Some({})", index(parent)) };
        let mut children: Vec<_> = commands
            .iter()
            .enumerate()
            .filter(|(_, o)| !o.path.is_empty() && o.path.rsplit_once(' ').map(|(p, _)| p).unwrap_or("") == c.path && o.path != c.path)
            .map(|(i, _)| i)
            .collect();
        children.sort_by(|a, b| commands[*a].use_line.cmp(&commands[*b].use_line));
        write!(
            out,
            "    Command {{ path: {:?}, use_line: {:?}, short: {:?}, long: {:?}, example: {:?}, aliases: &{:?}, hidden: {}, runnable: {}, parent: {parent}, children: &{children:?}, flags: &[",
            c.path, c.use_line, c.short, c.long, c.example, c.aliases, c.hidden, c.runnable
        )
        .unwrap();
        for f in &c.flag {
            let kind = match f.kind.as_str() {
                "bool" => "Bool",
                "string" => "String",
                "stringSlice" => "StringSlice",
                "int" => "Int",
                "int64" => "Int64",
                "uint64" => "Uint64",
                other => panic!("unknown flag type {other}"),
            };
            write!(
                out,
                "Flag {{ name: {:?}, short: {:?}, kind: FlagKind::{kind}, default: {:?}, no_opt_default: {:?}, usage: {:?}, hidden: {}, persistent: {}, required: {} }}, ",
                f.name, f.short, f.default, f.no_opt_default, f.usage, f.hidden, f.persistent, f.required
            )
            .unwrap();
        }
        write!(out, "], tool: ").unwrap();
        match &c.tool {
            None => write!(out, "None").unwrap(),
            Some(t) => {
                let effect = match t.effect.as_str() {
                    "read" => "Read",
                    "write" => "Write",
                    "destructive" => "Destructive",
                    "writes_local_file" => "WritesLocalFile",
                    other => panic!("unknown effect {other}"),
                };
                write!(
                    out,
                    "Some(Tool {{ name: {:?}, title: {:?}, description: {:?}, effect: Effect::{effect}, idempotent: {}, input_schema: {:?} }})",
                    t.name, t.title, t.description, t.idempotent, t.input_schema
                )
                .unwrap();
            }
        }
        writeln!(out, " }},").unwrap();
    }
    writeln!(out, "];").unwrap();
    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join("registry_gen.rs");
    std::fs::write(dest, out).unwrap();
}
