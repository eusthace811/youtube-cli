#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// spec/registry/*.toml, the source yutu-core compiles its registry from:
/// every command path, and every tool's (name, effect).
struct Registry {
    paths: Vec<String>,
    tools: Vec<(String, String)>,
}

fn registry() -> Registry {
    let mut reg = Registry {
        paths: Vec::new(),
        tools: Vec::new(),
    };
    for entry in fs::read_dir(repo().join("spec/registry")).unwrap() {
        let text = fs::read_to_string(entry.unwrap().path()).unwrap();
        let file: toml::Table = toml::from_str(&text).unwrap();
        for cmd in file["command"].as_array().unwrap() {
            reg.paths.push(cmd["path"].as_str().unwrap().to_string());
            if let Some(tool) = cmd.get("tool") {
                reg.tools.push((
                    tool["name"].as_str().unwrap().to_string(),
                    tool["effect"].as_str().unwrap().to_string(),
                ));
            }
        }
    }
    reg
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .into()
}

fn markdown_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            markdown_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
}

fn skill_files() -> Vec<(PathBuf, String)> {
    let mut paths = Vec::new();
    markdown_files(&repo().join("skills"), &mut paths);
    paths.sort();
    paths
        .into_iter()
        .map(|p| {
            let text = fs::read_to_string(&p).unwrap();
            (p, text)
        })
        .collect()
}

fn is_tool_like(s: &str) -> bool {
    let mut parts = s.split('-');
    let ident = |p: Option<&str>| {
        p.is_some_and(|p| {
            p.starts_with(|c: char| c.is_ascii_lowercase())
                && p.chars().all(|c| c.is_ascii_alphanumeric())
        })
    };
    ident(parts.next()) && ident(parts.next()) && parts.next().is_none()
}

#[test]
fn generated_skills_are_current() {
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["skillgen", "--check"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn every_named_tool_exists_in_the_registry() {
    let reg = registry();
    let tools: Vec<&str> = reg.tools.iter().map(|(name, _)| name.as_str()).collect();
    let mut named = 0;
    for (path, text) in skill_files() {
        for token in text.split('`').skip(1).step_by(2) {
            if is_tool_like(token) {
                assert!(
                    tools.contains(&token),
                    "{} names `{token}`, which is not a registry tool",
                    path.display()
                );
                named += 1;
            }
        }
    }
    // Both SKILL.md files list every tool.
    assert_eq!(named, 2 * tools.len());
    assert_eq!(tools.len(), 74);
}

#[test]
fn every_cli_example_names_a_registry_command() {
    let reg = registry();
    let exists = |path: &str| reg.paths.iter().any(|p| p == path);
    let mut checked = 0;
    for (path, text) in skill_files() {
        let mut in_code = false;
        for line in text.lines() {
            if line.trim_start().starts_with("```") {
                in_code = !in_code;
                continue;
            }
            if !in_code {
                continue;
            }
            // `$(yutu video list` and `| yutu ...` invoke yutu too.
            let words: Vec<&str> = line
                .split(|c: char| !(c.is_ascii_alphanumeric() || "-_/.@".contains(c)))
                .filter(|w| !w.is_empty())
                .collect();
            for (i, _) in words.iter().enumerate().filter(|(_, w)| **w == "yutu") {
                let cmd: Vec<&str> = words[i + 1..]
                    .iter()
                    .take(2)
                    .take_while(|w| w.chars().all(|c| c.is_ascii_alphanumeric()))
                    .copied()
                    .collect();
                if cmd.is_empty() {
                    continue;
                }
                let found = exists(&cmd.join(" ")) || exists(cmd[0]);
                assert!(
                    found,
                    "{}: `yutu {}` is not a command",
                    path.display(),
                    cmd.join(" ")
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 20, "only {checked} examples found");
}

/// The heading each effect's tools sit under in SKILL.md.
fn heading(effect: &str) -> &'static str {
    match effect {
        "read" => "### Read (safe, no side effects)",
        "write" => "### Write (create or update content)",
        "writes_local_file" => {
            "### Local file (writes a file on this machine — confirm the path with user first)"
        }
        "destructive" => "### Destructive (irreversible deletions — confirm with user first)",
        other => panic!("unknown effect {other}"),
    }
}

#[test]
fn each_tool_is_listed_under_its_registry_effect() {
    for skill in [
        "skills/youtube/SKILL.md",
        "skills/agent/skills/youtube/SKILL.md",
    ] {
        let text = fs::read_to_string(repo().join(skill)).unwrap();
        let mut section = "";
        let mut listed = Vec::new();
        for line in text.lines() {
            if line.starts_with("### ") {
                section = line;
            } else if let Some(name) = line.strip_prefix("- `").and_then(|l| l.strip_suffix('`')) {
                listed.push((name.to_string(), section));
            }
        }
        let expected: Vec<(String, &str)> = registry()
            .tools
            .into_iter()
            .map(|(name, effect)| (name, heading(&effect)))
            .collect();
        for entry in &expected {
            assert!(listed.contains(entry), "{skill}: {entry:?} missing");
        }
        assert_eq!(listed.len(), expected.len(), "{skill}");
        assert!(listed.contains(&(
            "caption-download".to_string(),
            "### Local file (writes a file on this machine — confirm the path with user first)"
        )));
    }
}
