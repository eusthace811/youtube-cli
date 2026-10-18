//! cobra's defaultHelpFunc and defaultUsageFunc, and pflag's FlagUsages.

use std::fmt::Write;

use super::flags::{Entry, Flags};
use super::goconv;
use super::tree::{self, Tree, command_path, kind, name};
use crate::registry::{Command, FlagKind};

/// What `--help` prints. `help_flag` says whether cobra has created the
/// command's `-h, --help` flag yet, which changes the use line and flags.
pub(crate) fn help(tree: Tree, cmd: &'static Command, help_flag: bool) -> String {
    let mut out = String::new();
    let text = goconv::trim_right_space(if cmd.long.is_empty() {
        cmd.short
    } else {
        cmd.long
    });
    if !text.is_empty() {
        out.push_str(text);
        out.push_str("\n\n");
    }
    if cmd.runnable || tree.has_subcommands(cmd) {
        out.push_str(&usage(tree, cmd, help_flag));
    }
    out
}

pub(crate) fn usage(tree: Tree, cmd: &'static Command, help_flag: bool) -> String {
    let flags = Flags::for_command(cmd, help_flag);
    let path = command_path(cmd);
    let has_available_subcommands = tree.has_available_subcommands(cmd);
    let mut w = String::from("Usage:");
    if cmd.runnable {
        write!(w, "\n  {}", use_line(cmd, &flags)).unwrap();
    }
    if has_available_subcommands {
        write!(w, "\n  {path} [command]").unwrap();
    }
    if !cmd.aliases.is_empty() {
        write!(
            w,
            "\n\nAliases:\n  {}, {}",
            name(cmd),
            cmd.aliases.join(", ")
        )
        .unwrap();
    }
    if !cmd.example.is_empty() {
        write!(w, "\n\nExamples:\n{}", cmd.example).unwrap();
    }
    if has_available_subcommands {
        w.push_str("\n\nAvailable Commands:");
        let padding = name_padding(tree, cmd);
        for sub in tree
            .children(cmd)
            .filter(|c| tree.is_available(c) || name(c) == "help")
        {
            write!(w, "\n  {} {}", rpad(name(sub), padding), sub.short).unwrap();
        }
    }
    let local: Vec<&Entry> = flags
        .entries()
        .iter()
        .filter(|e| !e.spec.inherited)
        .collect();
    let inherited: Vec<&Entry> = flags
        .entries()
        .iter()
        .filter(|e| e.spec.inherited)
        .collect();
    if local.iter().any(|e| !e.spec.hidden) {
        write!(
            w,
            "\n\nFlags:\n{}",
            goconv::trim_right_space(&flag_usages(&local))
        )
        .unwrap();
    }
    if inherited.iter().any(|e| !e.spec.hidden) {
        write!(
            w,
            "\n\nGlobal Flags:\n{}",
            goconv::trim_right_space(&flag_usages(&inherited))
        )
        .unwrap();
    }
    let topics: Vec<_> = tree
        .children(cmd)
        .filter(|c| tree.is_additional_help_topic(c))
        .collect();
    if !topics.is_empty() {
        w.push_str("\n\nAdditional help topics:");
        let padding = command_path_padding(tree, cmd);
        for sub in topics {
            write!(w, "\n  {} {}", rpad(&command_path(sub), padding), sub.short).unwrap();
        }
    }
    if has_available_subcommands {
        write!(
            w,
            "\n\nUse \"{path} [command] --help\" for more information about a command."
        )
        .unwrap();
    }
    w.push('\n');
    w
}

fn use_line(cmd: &'static Command, flags: &Flags) -> String {
    let mut line = match tree::parent(cmd) {
        Some(p) => format!("{} {}", command_path(p), cmd.use_line),
        None => cmd.use_line.to_string(),
    };
    if kind(cmd).disable_flags_in_use_line() {
        return line;
    }
    if flags.entries().iter().any(|e| !e.spec.hidden) && !line.contains("[flags]") {
        line.push_str(" [flags]");
    }
    line
}

/// cobra pads child names to the longest sibling name, at least 11.
fn name_padding(tree: Tree, cmd: &'static Command) -> usize {
    tree.children(cmd)
        .map(|c| name(c).len())
        .max()
        .unwrap_or(0)
        .max(11)
}

fn command_path_padding(tree: Tree, cmd: &'static Command) -> usize {
    tree.children(cmd)
        .map(|c| command_path(c).len())
        .max()
        .unwrap_or(0)
        .max(11)
}

fn rpad(s: &str, width: usize) -> String {
    format!("{s:<width$}")
}

/// pflag's UnquoteUsage: a back-quoted name in the usage, else the type.
fn unquote_usage(e: &Entry) -> (String, String) {
    let usage = e.spec.usage.as_ref();
    if let Some(start) = usage.find('`')
        && let Some(len) = usage[start + 1..].find('`')
    {
        let name = &usage[start + 1..start + 1 + len];
        return (
            name.to_string(),
            format!("{}{}{}", &usage[..start], name, &usage[start + 2 + len..]),
        );
    }
    let type_name = match e.spec.kind {
        FlagKind::Bool => "",
        FlagKind::String => "string",
        FlagKind::StringSlice => "strings",
        FlagKind::Int | FlagKind::Int64 => "int",
        FlagKind::Uint64 => "uint",
    };
    (type_name.to_string(), usage.to_string())
}

fn default_is_zero(e: &Entry) -> bool {
    let d = e.spec.default;
    match e.spec.kind {
        FlagKind::Bool => d == "false" || d.is_empty(),
        FlagKind::String => d.is_empty(),
        FlagKind::StringSlice => d == "[]",
        FlagKind::Int | FlagKind::Int64 | FlagKind::Uint64 => d == "0",
    }
}

/// pflag's FlagUsagesWrapped(0).
pub(crate) fn flag_usages(entries: &[&Entry]) -> String {
    let mut lines = Vec::new();
    let mut maxlen = 0;
    for e in entries.iter().filter(|e| !e.spec.hidden) {
        let spec = &e.spec;
        let mut line = if spec.short.is_empty() {
            format!("      --{}", spec.name)
        } else {
            format!("  -{}, --{}", spec.short, spec.name)
        };
        let (varname, usage) = unquote_usage(e);
        if !varname.is_empty() {
            line.push(' ');
            line.push_str(&varname);
        }
        if !spec.no_opt_default.is_empty() {
            match spec.kind {
                FlagKind::String => write!(line, "[=\"{}\"]", spec.no_opt_default).unwrap(),
                FlagKind::Bool if spec.no_opt_default == "true" => {}
                _ => write!(line, "[={}]", spec.no_opt_default).unwrap(),
            }
        }
        let sidx = line.len();
        maxlen = maxlen.max(sidx + 1);
        let mut text = usage;
        if !default_is_zero(e) {
            match spec.kind {
                FlagKind::String => {
                    write!(text, " (default {})", goconv::quote(spec.default)).unwrap()
                }
                _ => write!(text, " (default {})", spec.default).unwrap(),
            }
        }
        lines.push((line, text));
    }
    let mut out = String::new();
    for (prefix, text) in lines {
        let spacing = " ".repeat(maxlen - prefix.len());
        let text = text.replace('\n', &format!("\n{}", " ".repeat(maxlen + 2)));
        writeln!(out, "{prefix} {spacing} {text}").unwrap();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry;

    #[test]
    fn renders_cobra_help_for_a_leaf() {
        let tree = Tree { complete: false };
        let text = help(tree, registry::find("version").unwrap(), true);
        assert_eq!(
            text,
            "Show the version of yutu.\n\nUsage:\n  yutu version [flags]\n\nFlags:\n  -h, --help   help for version\n"
        );
    }

    #[test]
    fn root_usage_before_the_help_flag_exists_has_no_flags() {
        let tree = Tree { complete: false };
        let text = usage(tree, registry::root(), false);
        assert!(text.starts_with("Usage:\n  yutu\n  yutu [command]\n\nAvailable Commands:\n  abuseReport            Manage"));
        assert!(text.ends_with("  watermark              Manage YouTube watermarks\n\nUse \"yutu [command] --help\" for more information about a command.\n"));
    }

    #[test]
    fn flag_usages_align_and_show_defaults() {
        let flags = Flags::for_command(registry::find("video list").unwrap(), true);
        let entries: Vec<&Entry> = flags.entries().iter().collect();
        let text = flag_usages(&entries);
        assert!(text.contains("  -n, --maxResults int                  The maximum number of items that should be returned, 0 for no limit (default 5)\n"));
        assert!(text.contains(
            "  -o, --output string                   json|yaml|table (default \"table\")\n"
        ));
        assert!(text.contains("  -p, --parts strings                   Comma separated parts (default [id,snippet,status,statistics])\n"));
        assert!(text.contains("  -h, --help                            help for list\n"));
    }
}
