//! The command registry: one entry per CLI command, generated from
//! spec/registry/*.toml. Leaf commands that are MCP tools carry their tool
//! metadata, so the CLI tree, the MCP schemas and the docs share one source.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagKind {
    Bool,
    String,
    StringSlice,
    Int,
    Int64,
    Uint64,
}

#[derive(Debug)]
pub struct Flag {
    pub name: &'static str,
    pub short: &'static str,
    pub kind: FlagKind,
    pub default: &'static str,
    pub no_opt_default: &'static str,
    pub usage: &'static str,
    pub hidden: bool,
    pub persistent: bool,
    pub required: bool,
}

/// What an operation does to the world, from which the MCP annotations and
/// the confirmation gate derive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Read,
    Write,
    Destructive,
    WritesLocalFile,
}

#[derive(Debug)]
pub struct Tool {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub effect: Effect,
    pub idempotent: bool,
    pub input_schema: &'static str,
}

impl Tool {
    pub fn read_only(&self) -> bool {
        self.effect == Effect::Read
    }

    pub fn destructive(&self) -> bool {
        self.effect == Effect::Destructive
    }

    /// Whether a human must approve the call before it runs.
    pub fn gated(&self) -> bool {
        self.effect != Effect::Read
    }
}

#[derive(Debug)]
pub struct Command {
    /// Space-separated path below the root, empty for the root itself.
    pub path: &'static str,
    pub use_line: &'static str,
    pub short: &'static str,
    pub long: &'static str,
    pub example: &'static str,
    pub aliases: &'static [&'static str],
    pub hidden: bool,
    pub runnable: bool,
    pub parent: Option<usize>,
    /// Indices into [`COMMANDS`], sorted by name as cobra sorts them.
    pub children: &'static [usize],
    pub flags: &'static [Flag],
    pub tool: Option<Tool>,
}

impl Command {
    pub fn name(&self) -> &'static str {
        self.use_line.split_whitespace().next().unwrap_or("")
    }
}

include!(concat!(env!("OUT_DIR"), "/registry_gen.rs"));

pub fn root() -> &'static Command {
    find("").expect("root command")
}

pub fn find(path: &str) -> Option<&'static Command> {
    COMMANDS.binary_search_by(|c| c.path.cmp(path)).ok().map(|i| &COMMANDS[i])
}

pub fn tools() -> impl Iterator<Item = (&'static Command, &'static Tool)> {
    COMMANDS.iter().filter_map(|c| c.tool.as_ref().map(|t| (c, t)))
}
