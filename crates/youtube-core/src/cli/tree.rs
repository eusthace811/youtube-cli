//! The cobra command tree over the static registry: navigation, the
//! commands cobra adds at runtime, and Find with its legacy argument rules.

use super::goconv;
use crate::registry::{COMMANDS, Command, Flag};

/// cobra's hidden `__complete` command. It exists only while it is the
/// command being invoked, as cobra's initCompleteCmd arranges.
pub(crate) static COMPLETE: Command = Command {
    path: "__complete",
    use_line: "__complete [command-line]",
    short: "Request shell completion choices for the specified command-line",
    long: "__complete is a special command that is used by the shell completion logic\nto request completion choices for the specified command-line.",
    example: "",
    aliases: &["__completeNoDesc"],
    hidden: true,
    runnable: true,
    parent: Some(0),
    children: &[],
    flags: &[],
    tool: None,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shell {
    Bash,
    Zsh,
    Fish,
    Powershell,
}

/// What executing a command does. Leaves go to the [`super::App`]; the
/// rest are cobra built-ins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A runnable parent whose Run prints its own help.
    Group,
    Leaf,
    Help,
    CompletionGroup,
    Script(Shell),
    Complete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArgsRule {
    Legacy,
    NoArgs,
    MinimumOne,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValidArgs {
    None,
    NoFileCompletions,
    HelpTopics,
}

pub(crate) fn kind(cmd: &'static Command) -> Kind {
    if is(cmd, &COMPLETE) {
        return Kind::Complete;
    }
    match cmd.path {
        "help" => Kind::Help,
        "completion" => Kind::CompletionGroup,
        "completion bash" => Kind::Script(Shell::Bash),
        "completion zsh" => Kind::Script(Shell::Zsh),
        "completion fish" => Kind::Script(Shell::Fish),
        "completion powershell" => Kind::Script(Shell::Powershell),
        _ if !cmd.children.is_empty() => Kind::Group,
        _ => Kind::Leaf,
    }
}

impl Kind {
    pub fn args_rule(self) -> ArgsRule {
        match self {
            Kind::CompletionGroup | Kind::Script(_) => ArgsRule::NoArgs,
            Kind::Complete => ArgsRule::MinimumOne,
            _ => ArgsRule::Legacy,
        }
    }

    pub fn disable_flags_in_use_line(self) -> bool {
        matches!(self, Kind::Script(Shell::Bash) | Kind::Complete)
    }

    pub fn disable_flag_parsing(self) -> bool {
        self == Kind::Complete
    }

    pub fn valid_args(self) -> ValidArgs {
        match self {
            Kind::CompletionGroup | Kind::Script(_) => ValidArgs::NoFileCompletions,
            Kind::Help => ValidArgs::HelpTopics,
            _ => ValidArgs::None,
        }
    }
}

pub(crate) fn is(a: &Command, b: &Command) -> bool {
    std::ptr::eq(a, b)
}

pub(crate) fn root() -> &'static Command {
    &COMMANDS[0]
}

pub(crate) fn name(cmd: &Command) -> &'static str {
    cmd.name()
}

pub(crate) fn parent(cmd: &'static Command) -> Option<&'static Command> {
    cmd.parent.map(|i| &COMMANDS[i])
}

pub(crate) fn command_path(cmd: &'static Command) -> String {
    match parent(cmd) {
        Some(p) => format!("{} {}", command_path(p), name(cmd)),
        None => name(cmd).to_string(),
    }
}

/// The root's help command, which cobra hides from "available" listings.
pub(crate) fn is_help_command(cmd: &Command) -> bool {
    cmd.path == "help"
}

/// The registry flags a command parses: its own, then its ancestors'
/// persistent ones.
pub(crate) fn declared_flags(cmd: &'static Command) -> impl Iterator<Item = &'static Flag> {
    let ancestors = std::iter::successors(parent(cmd), |c| parent(c));
    cmd.flags
        .iter()
        .chain(ancestors.flat_map(|p| p.flags.iter().filter(|f| f.persistent)))
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Tree {
    /// Whether `__complete` is attached to the root for this run.
    pub complete: bool,
}

pub(crate) struct Found {
    pub cmd: &'static Command,
    pub args: Vec<String>,
    /// The name or alias the command was found by.
    pub called_as: String,
}

impl Tree {
    /// Children in cobra's sorted order.
    pub fn children(self, cmd: &'static Command) -> impl Iterator<Item = &'static Command> {
        let complete = (self.complete && cmd.parent.is_none()).then_some(&COMPLETE);
        complete
            .into_iter()
            .chain(cmd.children.iter().map(|&i| &COMMANDS[i]))
    }

    pub fn has_subcommands(self, cmd: &'static Command) -> bool {
        self.children(cmd).next().is_some()
    }

    pub fn is_available(self, cmd: &'static Command) -> bool {
        !cmd.hidden
            && !is_help_command(cmd)
            && (cmd.runnable || self.has_available_subcommands(cmd))
    }

    pub fn has_available_subcommands(self, cmd: &'static Command) -> bool {
        self.children(cmd).any(|c| self.is_available(c))
    }

    pub fn is_additional_help_topic(self, cmd: &'static Command) -> bool {
        !cmd.runnable && !cmd.hidden && self.children(cmd).all(|c| self.is_additional_help_topic(c))
    }

    fn find_next(self, cmd: &'static Command, word: &str) -> Option<&'static Command> {
        self.children(cmd)
            .find(|c| name(c) == word || c.aliases.contains(&word))
    }

    /// cobra's Command.Find from the root.
    pub fn find(self, args: &[String]) -> (Found, Result<(), String>) {
        let mut cmd = root();
        let mut args = args.to_vec();
        let mut called_as = name(cmd).to_string();
        loop {
            let words = strip_flags(&args, cmd);
            let Some(word) = words.first() else { break };
            let Some(next) = self.find_next(cmd, word) else {
                break;
            };
            args = args_minus_first_x(&args, word, cmd);
            called_as = word.clone();
            cmd = next;
        }
        let checked = match kind(cmd).args_rule() {
            ArgsRule::Legacy => self.legacy_args(cmd, &strip_flags(&args, cmd)),
            _ => Ok(()),
        };
        (
            Found {
                cmd,
                args,
                called_as,
            },
            checked,
        )
    }

    fn legacy_args(self, cmd: &'static Command, args: &[String]) -> Result<(), String> {
        if !self.has_subcommands(cmd) || parent(cmd).is_some() || args.is_empty() {
            return Ok(());
        }
        Err(format!(
            "unknown command {} for {}{}",
            goconv::quote(&args[0]),
            goconv::quote(&command_path(cmd)),
            self.suggestions(cmd, &args[0])
        ))
    }

    fn suggestions(self, cmd: &'static Command, typed: &str) -> String {
        let typed_lower = typed.to_lowercase();
        let found: Vec<&str> = registration_order(self, cmd)
            .into_iter()
            .filter(|c| self.is_available(c))
            .map(|c| name(c))
            .filter(|n| {
                levenshtein(&typed_lower, &n.to_lowercase()) <= 2
                    || n.to_lowercase().starts_with(&typed_lower)
            })
            .collect();
        if found.is_empty() {
            return String::new();
        }
        let mut out = String::from("\n\nDid you mean this?\n");
        for n in found {
            out.push('\t');
            out.push_str(n);
            out.push('\n');
        }
        out
    }
}

/// The order Go's yutu adds root subcommands, which cobra's suggestions
/// follow: package cmd's own init (auth, mcp, version) runs before the
/// resource packages, and cobra appends help and completion at execution.
fn registration_order(tree: Tree, cmd: &'static Command) -> Vec<&'static Command> {
    const FIRST: [&str; 3] = ["auth", "mcp", "version"];
    const LAST: [&str; 2] = ["help", "completion"];
    let children: Vec<_> = tree.children(cmd).collect();
    if parent(cmd).is_some() {
        return children;
    }
    let by_name = |n: &str| children.iter().copied().find(|c| name(c) == n);
    let middle = children
        .iter()
        .copied()
        .filter(|c| !FIRST.contains(&name(c)) && !LAST.contains(&name(c)));
    FIRST
        .iter()
        .filter_map(|n| by_name(n))
        .chain(middle)
        .chain(LAST.iter().filter_map(|n| by_name(n)))
        .collect()
}

fn levenshtein(s: &str, t: &str) -> usize {
    let (s, t) = (s.as_bytes(), t.as_bytes());
    let mut prev: Vec<usize> = (0..=s.len()).collect();
    for j in 1..=t.len() {
        let mut cur = vec![j; s.len() + 1];
        for i in 1..=s.len() {
            cur[i] = if s[i - 1] == t[j - 1] {
                prev[i - 1]
            } else {
                1 + prev[i].min(cur[i - 1]).min(prev[i - 1])
            };
        }
        prev = cur;
    }
    prev[s.len()]
}

fn has_no_opt_default(cmd: &'static Command, name: &str) -> bool {
    declared_flags(cmd).any(|f| f.name == name && !f.no_opt_default.is_empty())
}

fn short_has_no_opt_default(cmd: &'static Command, name: &str) -> bool {
    let Some(&c) = name.as_bytes().first() else {
        return false;
    };
    declared_flags(cmd).any(|f| f.short.as_bytes() == [c] && !f.no_opt_default.is_empty())
}

/// Whether `s` is a flag that consumes the next argument, as cobra's
/// stripFlags and argsMinusFirstX guess it before parsing.
fn takes_next_arg(s: &str, cmd: &'static Command) -> bool {
    if s.contains('=') {
        return false;
    }
    (s.starts_with("--") && !has_no_opt_default(cmd, &s[2..]))
        || (s.starts_with('-') && s.len() == 2 && !short_has_no_opt_default(cmd, &s[1..]))
}

fn strip_flags(args: &[String], cmd: &'static Command) -> Vec<String> {
    let mut commands = Vec::new();
    let mut rest = args;
    while let Some((s, tail)) = rest.split_first() {
        rest = tail;
        if s == "--" {
            break;
        }
        if takes_next_arg(s, cmd) {
            if rest.len() <= 1 {
                break;
            }
            rest = &rest[1..];
        } else if !s.is_empty() && !s.starts_with('-') {
            commands.push(s.clone());
        }
    }
    commands
}

fn args_minus_first_x(args: &[String], x: &str, cmd: &'static Command) -> Vec<String> {
    let mut pos = 0;
    while pos < args.len() {
        let s = &args[pos];
        if s == "--" {
            break;
        }
        if takes_next_arg(s, cmd) {
            pos += 2;
            continue;
        }
        if !s.starts_with('-') && s == x {
            let mut out = args.to_vec();
            out.remove(pos);
            return out;
        }
        pos += 1;
    }
    args.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    fn find(args: &[&str]) -> (String, Vec<String>, Result<(), String>) {
        let (found, checked) = Tree { complete: false }.find(&strings(args));
        (command_path(found.cmd), found.args, checked)
    }

    #[test]
    fn complete_hangs_off_the_root() {
        assert_eq!(command_path(&COMPLETE), "youtube __complete");
        assert_eq!(root().path, "");
    }

    #[test]
    fn find_walks_subcommands_around_flags() {
        assert_eq!(
            find(&["video", "list", "-o", "json"]),
            (
                "youtube video list".into(),
                strings(&["-o", "json"]),
                Ok(())
            )
        );
        assert_eq!(
            find(&["-h", "video"]),
            ("youtube".into(), strings(&["-h", "video"]), Ok(()))
        );
        assert_eq!(
            find(&["video", "-h", "list"]),
            ("youtube video".into(), strings(&["-h", "list"]), Ok(()))
        );
        assert_eq!(
            find(&["video", "nope"]),
            ("youtube video".into(), strings(&["nope"]), Ok(()))
        );
        assert_eq!(
            find(&["playlist", "--mine", "list"]),
            (
                "youtube playlist".into(),
                strings(&["--mine", "list"]),
                Ok(())
            )
        );
    }

    #[test]
    fn unknown_root_commands_get_suggestions() {
        let (_, _, checked) = find(&["vidoe"]);
        assert_eq!(
            checked,
            Err(
                "unknown command \"vidoe\" for \"youtube\"\n\nDid you mean this?\n\tvideo\n".into()
            )
        );
        let (_, _, checked) = find(&["c"]);
        assert_eq!(
            checked.unwrap_err(),
            "unknown command \"c\" for \"youtube\"\n\nDid you mean this?\n\tmcp\n\tcaption\n\tchannel\n\tchannelBanner\n\tchannelSection\n\tcomment\n\tcommentThread\n\tcompletion\n"
        );
        let (_, _, checked) = find(&["he"]);
        assert_eq!(
            checked,
            Err("unknown command \"he\" for \"youtube\"".into())
        );
    }

    #[test]
    fn levenshtein_matches_cobra() {
        assert_eq!(levenshtein("vidoe", "video"), 2);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
    }
}
