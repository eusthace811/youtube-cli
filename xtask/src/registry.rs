//! The slice of spec/registry/*.toml the generators need: command paths,
//! short help, and each tool's name and effect. yutu-core's build script
//! reads the same files, so the generators and the binary share one source.

use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    Read,
    Write,
    Destructive,
    WritesLocalFile,
}

#[derive(Debug, Deserialize)]
pub struct Tool {
    pub name: String,
    pub effect: Effect,
}

#[derive(Debug, Deserialize)]
pub struct Command {
    pub path: String,
    #[serde(default)]
    pub short: String,
    pub tool: Option<Tool>,
}

impl Command {
    pub fn name(&self) -> &str {
        self.path.rsplit(' ').next().unwrap_or("")
    }
}

#[derive(Deserialize)]
struct File {
    command: Vec<Command>,
}

pub struct Registry {
    /// Every command, sorted by path.
    pub commands: Vec<Command>,
}

impl Registry {
    pub fn load(root: &Path) -> Result<Self, String> {
        let dir = root.join("spec/registry");
        let mut commands = Vec::new();
        for entry in fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.extension().is_some_and(|e| e == "toml") {
                let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
                let file: File =
                    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
                commands.extend(file.command);
            }
        }
        commands.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Self { commands })
    }

    /// Direct children of `parent` ("" for the root), sorted by name.
    pub fn children<'a>(&'a self, parent: &str) -> Vec<&'a Command> {
        self.commands
            .iter()
            .filter(|c| match parent {
                "" => !c.path.is_empty() && !c.path.contains(' '),
                _ => c
                    .path
                    .strip_prefix(parent)
                    .and_then(|rest| rest.strip_prefix(' '))
                    .is_some_and(|rest| !rest.contains(' ')),
            })
            .collect()
    }

    /// Top-level commands that manage a YouTube resource. Go's generators
    /// detect them by the "Manage" prefix of their short help.
    pub fn resources(&self) -> Vec<&Command> {
        self.children("")
            .into_iter()
            .filter(|c| c.short.starts_with("Manage"))
            .collect()
    }
}
