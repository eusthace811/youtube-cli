//! ADK's skill toolset over the embedded `assets/skills` tree, preloaded:
//! `list_skills`, `load_skill` and `load_skill_resource` with ADK's schemas,
//! argument validation, results and error texts, plus the instruction block
//! the toolset appends to the system prompt.

use serde_json::{Map, Value, json};

use crate::model::Declaration;

const SKILL_INSTRUCTION: &str = "You can use specialized 'skills' to help you with complex tasks. You MUST use the skill tools to interact with these skills.

Skills are folders of instructions and resources that extend your capabilities for specialized tasks. Each skill folder contains:
- **SKILL.md** (required): The main instruction file with skill metadata and detailed markdown instructions.
- **references/** (Optional): Additional documentation or examples for skill usage.
- **assets/** (Optional): Templates, scripts or other resources used by the skill.
- **scripts/** (Optional): Executable scripts that can be run via bash.

This is very important:

1. If a skill seems relevant to the current user query, you MUST use the `load_skill` tool with `name=\"<SKILL_NAME>\"` to read its full instructions before proceeding.
2. Once you have read the instructions, follow them exactly as documented before replying to the user. For example, If the instruction lists multiple steps, please make sure you complete all of them in order.
3. The `load_skill_resource` tool is for viewing files within a skill's directory (e.g., `references/*`, `assets/*`, `scripts/*`). Do NOT use other tools to access these files.
";

/// The embedded tree: skill directory name, SKILL.md, and resources by path
/// relative to the skill directory, sorted.
type Resources = &'static [(&'static str, &'static str)];

const EMBEDDED: &[(&str, &str, Resources)] = &[(
    "youtube",
    include_str!("../assets/skills/youtube/SKILL.md"),
    &[
        (
            "references/seo-guide.md",
            include_str!("../assets/skills/youtube/references/seo-guide.md"),
        ),
        (
            "references/workflows.md",
            include_str!("../assets/skills/youtube/references/workflows.md"),
        ),
    ],
)];

#[derive(Debug, Clone, PartialEq)]
pub struct Frontmatter {
    pub name: String,
    pub description: String,
    pub license: String,
    pub compatibility: String,
    pub metadata: Vec<(String, String)>,
    pub allowed_tools: Vec<String>,
}

pub struct Skill {
    pub frontmatter: Frontmatter,
    pub instructions: &'static str,
    resources: Resources,
}

pub struct Skills {
    /// Sorted by name, as ADK's complete preload sorts them.
    skills: Vec<Skill>,
}

pub const TOOL_NAMES: [&str; 3] = ["list_skills", "load_skill", "load_skill_resource"];

impl Skills {
    /// The skills the agent can see, which is none, as in Go: yutu's embed
    /// nests the skill one directory deeper than ADK's toolset reads, so
    /// `list_skills` is empty and `load_skill` finds nothing. Kept for parity
    /// (a known Go defect in docs/PARITY.md); `embedded()` is the fix.
    pub fn visible() -> Skills {
        Skills { skills: Vec::new() }
    }

    pub fn embedded() -> Skills {
        let mut skills: Vec<Skill> = EMBEDDED
            .iter()
            .map(|(dir, text, resources)| {
                let (frontmatter, instructions) =
                    parse(text).unwrap_or_else(|e| panic!("embedded skill {dir}: {e}"));
                assert_eq!(&frontmatter.name, dir, "skill name matches its directory");
                Skill {
                    frontmatter,
                    instructions,
                    resources,
                }
            })
            .collect();
        skills.sort_by(|a, b| a.frontmatter.name.cmp(&b.frontmatter.name));
        Skills { skills }
    }

    /// The toolset's system instruction block, or nothing when no skill
    /// loaded, as ADK's `ProcessRequest` does.
    pub fn instruction(&self) -> Option<String> {
        if self.skills.is_empty() {
            return None;
        }
        Some(format!("{SKILL_INSTRUCTION}\n\n{}", self.xml()))
    }

    fn xml(&self) -> String {
        let mut s = String::from("<available_skills>\n");
        for skill in &self.skills {
            s.push_str("<skill>\n<name>\n");
            s.push_str(&html_escape(&skill.frontmatter.name));
            s.push_str("\n</name>\n<description>\n");
            s.push_str(&html_escape(&skill.frontmatter.description));
            s.push_str("\n</description>\n</skill>\n");
        }
        s.push_str("</available_skills>");
        s
    }

    fn find(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.frontmatter.name == name)
    }

    pub fn declarations() -> Vec<Declaration> {
        let string = |d: &str| json!({"type": "string", "description": d});
        vec![
            Declaration {
                name: "list_skills".into(),
                description: "Lists all available skills with their names and descriptions.".into(),
                parameters: json!({"type": "object", "additionalProperties": false}),
                response: Some(json!({
                    "type": ["null", "object"],
                    "properties": {"skills": {"type": "string"}},
                    "required": ["skills"],
                    "additionalProperties": false,
                })),
            },
            Declaration {
                name: "load_skill".into(),
                description: "Loads the SKILL.md instructions for a given skill.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {"name": string("The name of the skill to load.")},
                    "required": ["name"],
                    "additionalProperties": false,
                }),
                response: Some(json!({
                    "type": ["null", "object"],
                    "properties": {
                        "skill_name": {"type": "string"},
                        "instructions": {"type": "string"},
                        "frontmatter": {
                            "type": ["null", "object"],
                            "properties": {
                                "name": {"type": "string"},
                                "description": {"type": "string"},
                                "license": {"type": "string"},
                                "compatibility": {"type": "string"},
                                "metadata": {"type": "object", "additionalProperties": {"type": "string"}},
                                "allowed-tools": {"type": ["null", "array"], "items": {"type": "string"}},
                            },
                            "required": ["name", "description"],
                            "additionalProperties": false,
                        },
                    },
                    "additionalProperties": false,
                })),
            },
            Declaration {
                name: "load_skill_resource".into(),
                description: "Loads a resource file (e.g., from references/ or assets/) associated with the specified skill.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "skill_name": string("The name of the skill."),
                        "resource_path": string("The relative path to the resource (e.g., 'references/my_doc.md', 'assets/template.txt', or 'scripts/setup.sh')."),
                    },
                    "required": ["skill_name", "resource_path"],
                    "additionalProperties": false,
                }),
                response: Some(json!({
                    "type": ["null", "object"],
                    "properties": {
                        "skill_name": {"type": "string"},
                        "path": {"type": "string"},
                        "content": {"type": "string"},
                    },
                    "additionalProperties": false,
                })),
            },
        ]
    }

    /// Runs one skill tool; the error is the text ADK puts under `error`.
    pub fn call(
        &self,
        tool: &str,
        args: &Map<String, Value>,
    ) -> Result<Map<String, Value>, String> {
        match tool {
            "list_skills" => {
                validate(args, &[])?;
                Ok(object([("skills", Value::String(self.xml()))]))
            }
            "load_skill" => {
                validate(args, &["name"])?;
                let name = str_arg(args, "name");
                if name.is_empty() {
                    return Err("skill name is required to load a skill".into());
                }
                let skill = self.find(name).ok_or_else(|| {
                    format!(
                        "load frontmatter for skill {}: skill not found",
                        yutu_auth::log::quote(name)
                    )
                })?;
                let mut out = Map::new();
                out.insert("skill_name".into(), name.into());
                if !skill.instructions.is_empty() {
                    out.insert("instructions".into(), skill.instructions.into());
                }
                out.insert("frontmatter".into(), frontmatter_json(&skill.frontmatter));
                Ok(out)
            }
            "load_skill_resource" => {
                validate(args, &["skill_name", "resource_path"])?;
                let name = str_arg(args, "skill_name");
                let path = str_arg(args, "resource_path");
                if name.is_empty() {
                    return Err("skill name is required to load a resource".into());
                }
                if path.is_empty() {
                    return Err(format!(
                        "resource path is required to load a resource for skill {}",
                        yutu_auth::log::quote(name)
                    ));
                }
                let fail = |why: &str| format!("load resource '{path}' from skill '{name}': {why}");
                let skill = self.find(name).ok_or_else(|| fail("skill not found"))?;
                let clean = clean_path(path);
                let content = skill
                    .resources
                    .iter()
                    .find(|(p, _)| *p == clean)
                    .map(|(_, c)| *c)
                    .ok_or_else(|| fail("resource not found"))?;
                let mut out = Map::new();
                out.insert("skill_name".into(), name.into());
                out.insert("path".into(), path.into());
                if !content.is_empty() {
                    out.insert("content".into(), content.into());
                }
                Ok(out)
            }
            _ => unreachable!("not a skill tool: {tool}"),
        }
    }
}

fn object<const N: usize>(fields: [(&str, Value); N]) -> Map<String, Value> {
    fields
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

fn str_arg<'a>(args: &'a Map<String, Value>, key: &str) -> &'a str {
    args.get(key).and_then(Value::as_str).unwrap_or("")
}

fn frontmatter_json(f: &Frontmatter) -> Value {
    let mut m = Map::new();
    m.insert("name".into(), f.name.clone().into());
    m.insert("description".into(), f.description.clone().into());
    if !f.license.is_empty() {
        m.insert("license".into(), f.license.clone().into());
    }
    if !f.compatibility.is_empty() {
        m.insert("compatibility".into(), f.compatibility.clone().into());
    }
    if !f.metadata.is_empty() {
        let meta: Map<String, Value> = f
            .metadata
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        m.insert("metadata".into(), Value::Object(meta));
    }
    if !f.allowed_tools.is_empty() {
        m.insert("allowed-tools".into(), json!(f.allowed_tools));
    }
    Value::Object(m)
}

/// jsonschema-go's verdict on a schema of string properties with
/// `additionalProperties: false`, in its order: property types, extra
/// properties, then required ones.
fn validate(args: &Map<String, Value>, props: &[&str]) -> Result<(), String> {
    for p in props {
        if let Some(v) = args.get(*p).filter(|v| !v.is_string()) {
            return Err(format!(
                "validating root: validating /properties/{p}: type: {} has type \"{}\", want \"string\"",
                go_v(v, true),
                json_type(v)
            ));
        }
    }
    let mut extra: Vec<&String> = args
        .keys()
        .filter(|k| !props.contains(&k.as_str()))
        .collect();
    if !extra.is_empty() {
        extra.sort();
        return Err(format!(
            "validating root: unexpected additional properties {}",
            go_quoted_list(extra.iter().map(|s| s.as_str()))
        ));
    }
    let missing: Vec<&str> = props
        .iter()
        .copied()
        .filter(|p| !args.contains_key(*p))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "validating root: required: missing properties: {}",
            go_quoted_list(missing.into_iter())
        ));
    }
    Ok(())
}

fn json_type(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.as_f64().is_some_and(|f| f.fract() == 0.0) => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Go's `%q` of a `[]string`.
fn go_quoted_list<'a>(items: impl Iterator<Item = &'a str>) -> String {
    let quoted: Vec<String> = items.map(yutu_auth::log::quote).collect();
    format!("[{}]", quoted.join(" "))
}

/// Go's `%v` of a decoded JSON value; a top-level null is the invalid
/// reflect.Value jsonschema-go formats.
fn go_v(v: &Value, top: bool) -> String {
    match v {
        Value::Null if top => "<invalid reflect.Value>".into(),
        Value::Null => "<nil>".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => yutu_api::json::format_float(n.as_f64().unwrap_or(0.0)),
        Value::String(s) => s.clone(),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(|i| go_v(i, false)).collect();
            format!("[{}]", inner.join(" "))
        }
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let inner: Vec<String> = keys
                .into_iter()
                .map(|k| format!("{k}:{}", go_v(&m[k], false)))
                .collect();
            format!("map[{}]", inner.join(" "))
        }
    }
}

/// Go's `html.EscapeString`.
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '\'' => out.push_str("&#39;"),
            '"' => out.push_str("&#34;"),
            c => out.push(c),
        }
    }
    out
}

/// Go's `path.Clean`.
fn clean_path(p: &str) -> String {
    let rooted = p.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|l| *l != "..") {
                    parts.pop();
                } else if !rooted {
                    parts.push("..");
                }
            }
            s => parts.push(s),
        }
    }
    let joined = parts.join("/");
    match (rooted, joined.is_empty()) {
        (true, _) => format!("/{joined}"),
        (false, true) => ".".into(),
        (false, false) => joined,
    }
}

/// ADK's frontmatter parse of the subset the embedded skills use: a `---`
/// block of `key: value` lines, quoted or plain, with one level of `metadata`
/// mapping. Returns the frontmatter and the instructions after the block.
fn parse(text: &'static str) -> Result<(Frontmatter, &'static str), String> {
    let body = text
        .strip_prefix("---\n")
        .ok_or("invalid frontmatter separator line: must be '---' followed by a new line")?;
    let end = body.find("\n---\n").ok_or(
        "missing closing frontmatter separator line: must be '---' followed by a new line",
    )?;
    let (yaml, instructions) = (&body[..end + 1], &body[end + 5..]);
    let mut f = Frontmatter {
        name: String::new(),
        description: String::new(),
        license: String::new(),
        compatibility: String::new(),
        metadata: Vec::new(),
        allowed_tools: Vec::new(),
    };
    let mut in_metadata = false;
    for line in yaml.lines() {
        if let Some(nested) = line.strip_prefix("  ") {
            if !in_metadata {
                return Err(format!("unsupported frontmatter line {line:?}"));
            }
            let (k, v) = key_value(nested)?;
            f.metadata.push((k.to_string(), v));
            continue;
        }
        let (k, v) = key_value(line)?;
        in_metadata = k == "metadata";
        match k {
            "name" => f.name = v,
            "description" => f.description = v,
            "license" => f.license = v,
            "compatibility" => f.compatibility = v,
            "allowed-tools" => {
                f.allowed_tools = v
                    .split(|c: char| c.is_whitespace() || c == ',')
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect()
            }
            "metadata" if v.is_empty() => {}
            _ => return Err(format!("unsupported frontmatter key {k:?}")),
        }
    }
    f.metadata.sort();
    Ok((f, instructions))
}

fn key_value(line: &str) -> Result<(&str, String), String> {
    let (k, v) = line
        .split_once(':')
        .ok_or_else(|| format!("unsupported frontmatter line {line:?}"))?;
    let v = v.trim();
    let v = match v.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        Some(inner) => serde_json::from_str::<String>(&format!("\"{inner}\""))
            .map_err(|e| format!("frontmatter value {v}: {e}"))?,
        None => v.to_string(),
    };
    Ok((k.trim(), v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_youtube_skill_parses() {
        let skills = Skills::embedded();
        let skill = skills.find("youtube").unwrap();
        assert_eq!(skill.frontmatter.license, "Apache-2.0");
        assert_eq!(
            skill.frontmatter.metadata,
            vec![
                ("author".to_string(), "eat-pray-ai".to_string()),
                (
                    "homepage".to_string(),
                    "https://github.com/eat-pray-ai/yutu".to_string()
                ),
            ]
        );
        assert!(skill.instructions.starts_with("\n# YouTube\n"));
    }

    #[test]
    fn resource_paths_clean_like_go() {
        assert_eq!(
            clean_path("references/./seo-guide.md"),
            "references/seo-guide.md"
        );
        assert_eq!(clean_path("../SKILL.md"), "../SKILL.md");
        assert_eq!(clean_path("references/../x"), "x");
    }

    #[test]
    fn validation_errors_match_jsonschema_go() {
        let skills = Skills::embedded();
        let call =
            |tool: &str, args: Value| skills.call(tool, args.as_object().unwrap()).unwrap_err();
        assert_eq!(
            call("load_skill", json!({"x": 1})),
            r#"validating root: unexpected additional properties ["x"]"#
        );
        assert_eq!(
            call("load_skill_resource", json!({})),
            r#"validating root: required: missing properties: ["skill_name" "resource_path"]"#
        );
        assert_eq!(
            call("load_skill", json!({"name": [1, "a", {"b": true}]})),
            r#"validating root: validating /properties/name: type: [1 a map[b:true]] has type "array", want "string""#
        );
        assert_eq!(
            call("load_skill", json!({"name": null})),
            r#"validating root: validating /properties/name: type: <invalid reflect.Value> has type "null", want "string""#
        );
        assert_eq!(
            call(
                "load_skill_resource",
                json!({"skill_name": "youtube", "resource_path": "../SKILL.md"})
            ),
            "load resource '../SKILL.md' from skill 'youtube': resource not found"
        );
    }
}
