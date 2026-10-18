//! Shell completion: the `completion <shell>` scripts and cobra's hidden
//! `__complete` protocol the scripts call back into.

use std::fmt::Write as _;
use std::io::Write;

use super::flags::{Entry, Flags};
use super::goconv;
use super::tree::{self, Shell, Tree, ValidArgs, kind, name};
use crate::registry::{Command, FlagKind};

/// cobra v1.10.2's generated scripts for a root command named `yutu`. Each
/// names the `__complete` request command exactly once.
pub(crate) fn script(shell: Shell, descriptions: bool) -> String {
    let text = match shell {
        Shell::Bash => include_str!("scripts/bash.bash"),
        Shell::Zsh => include_str!("scripts/zsh.zsh"),
        Shell::Fish => include_str!("scripts/fish.fish"),
        Shell::Powershell => include_str!("scripts/powershell.ps1"),
    };
    if descriptions {
        text.to_string()
    } else {
        text.replacen("__complete", "__completeNoDesc", 1)
    }
}

const ERROR: u32 = 1;
const NO_SPACE: u32 = 2;
const NO_FILE_COMP: u32 = 4;
const FILTER_FILE_EXT: u32 = 8;
const FILTER_DIRS: u32 = 16;
const KEEP_ORDER: u32 = 32;

fn directive_names(d: u32) -> String {
    let names: Vec<&str> = [
        (ERROR, "ShellCompDirectiveError"),
        (NO_SPACE, "ShellCompDirectiveNoSpace"),
        (NO_FILE_COMP, "ShellCompDirectiveNoFileComp"),
        (FILTER_FILE_EXT, "ShellCompDirectiveFilterFileExt"),
        (FILTER_DIRS, "ShellCompDirectiveFilterDirs"),
        (KEEP_ORDER, "ShellCompDirectiveKeepOrder"),
    ]
    .into_iter()
    .filter(|(bit, _)| d & bit != 0)
    .map(|(_, n)| n)
    .collect();
    if names.is_empty() {
        "ShellCompDirectiveDefault".to_string()
    } else {
        names.join(", ")
    }
}

/// Go's `%v` of a []string.
fn go_list(args: &[String]) -> String {
    format!("[{}]", args.join(" "))
}

/// The `__complete` command's Run.
pub(crate) fn run(args: &[String], called_as: &str, out: &mut dyn Write, err: &mut dyn Write) {
    let (completions, directive, error) = completions(args);
    if let Some(e) = error {
        let msg = format!("[Debug] [Error] {e}\n");
        if let Some(path) = std::env::var_os("BASH_COMP_DEBUG_FILE").filter(|p| !p.is_empty())
            && let Ok(mut f) = std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(path)
        {
            let _ = f.write_all(msg.as_bytes());
        }
        let _ = err.write_all(msg.as_bytes());
    }

    let mut no_descriptions = called_as == "__completeNoDesc";
    if !no_descriptions && let Ok(want) = goconv::parse_bool(&env_config("COMPLETION_DESCRIPTIONS"))
    {
        no_descriptions = !want;
    }
    let mut text = String::new();
    for comp in completions {
        let comp = if no_descriptions {
            comp.split('\t').next().unwrap_or("")
        } else {
            comp.as_str()
        };
        let comp = comp.split('\n').next().unwrap_or("");
        writeln!(text, "{}", comp.trim_matches(char::is_whitespace)).unwrap();
    }
    writeln!(text, ":{directive}").unwrap();
    let _ = out.write_all(text.as_bytes());
    let _ = writeln!(
        err,
        "Completion ended with directive: {}",
        directive_names(directive)
    );
}

/// `YUTU_<suffix>`, falling back to `COBRA_<suffix>` when empty.
fn env_config(suffix: &str) -> String {
    let get = |k: String| {
        std::env::var_os(k)
            .map(|v| v.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let v = get(format!("YUTU_{suffix}"));
    if v.is_empty() {
        get(format!("COBRA_{suffix}"))
    } else {
        v
    }
}

fn with_desc(choice: &str, description: &str) -> String {
    format!("{choice}\t{description}")
}

/// cobra's getCompletions for `args`, whose last element is the word
/// being completed.
fn completions(args: &[String]) -> (Vec<String>, u32, Option<String>) {
    let tree = Tree { complete: true };
    let (to_complete, trimmed) = args.split_last().expect("__complete requires an argument");
    let (found, checked) = tree.find(trimmed);
    if checked.is_err() {
        return (
            Vec::new(),
            0,
            Some(format!(
                "unable to find a command for arguments: {}",
                go_list(trimmed)
            )),
        );
    }
    let cmd = found.cmd;
    let parses = !kind(cmd).disable_flag_parsing();
    // cobra adds the help flag here unless flag parsing is off, and
    // __complete, the only such command, already has it from executing.
    let mut flags = Flags::for_command(cmd, true);

    let (flag, final_args, to_complete, flag_err) =
        check_flag_completion(cmd, &flags, found.args, to_complete);

    let mut with_dash = final_args.clone();
    with_dash.push("--".to_string());
    let dash_count = if parses {
        flags.parse(&with_dash).map_or(0, |a| a.len())
    } else {
        0
    };
    let positional = if parses {
        match flags.parse(&final_args) {
            Ok(p) => p,
            Err(e) => {
                let msg = format!(
                    "Error while parsing flags from args {}: {e}",
                    go_list(&final_args)
                );
                return (Vec::new(), 0, Some(msg));
            }
        }
    } else {
        final_args.clone()
    };
    let flag_completion = !(parses && dash_count > positional.len());
    if let Some(e) = flag_err
        && flag_completion
    {
        return (Vec::new(), 0, Some(e));
    }
    if flags.lookup("help").is_some_and(|e| e.changed) {
        return (Vec::new(), NO_FILE_COMP, None);
    }
    let final_args = if parses { positional } else { final_args };

    let mut completions = Vec::new();
    let directive;
    if flag.is_none()
        && to_complete.starts_with('-')
        && !to_complete.contains('=')
        && flag_completion
    {
        completions = required_flag_completions(&flags, &to_complete);
        if completions.is_empty() {
            for e in ordered(&flags) {
                if !e.changed || e.spec.kind == FlagKind::StringSlice {
                    completions.extend(flag_name_completions(e, &to_complete));
                }
            }
        }
        directive = if completions.len() == 1 && completions[0].ends_with('=') {
            NO_SPACE
        } else {
            NO_FILE_COMP
        };
        if parses {
            return (completions, directive, None);
        }
    } else {
        let mut d = 0;
        if flag.is_none() {
            let local_changed = flags
                .entries()
                .iter()
                .any(|e| !e.spec.inherited && !e.spec.persistent && e.changed);
            if final_args.is_empty() && !local_changed {
                for sub in tree.children(cmd) {
                    if tree.is_available(sub) || is_root_help(cmd, sub) {
                        if name(sub).starts_with(to_complete.as_str()) {
                            completions.push(with_desc(name(sub), sub.short));
                        }
                        d = NO_FILE_COMP;
                    }
                }
            }
            completions.extend(required_flag_completions(&flags, &to_complete));
        }
        directive = d;
    }

    let use_valid_args = flag.is_none() || !flag_completion;
    let directive = match kind(cmd).valid_args() {
        ValidArgs::NoFileCompletions if use_valid_args => NO_FILE_COMP,
        ValidArgs::HelpTopics if use_valid_args => {
            completions.extend(help_topics(tree, &final_args, &to_complete));
            NO_FILE_COMP
        }
        _ => directive,
    };
    (completions, directive, None)
}

fn is_root_help(parent: &Command, sub: &Command) -> bool {
    parent.parent.is_none() && tree::is_help_command(sub)
}

/// Inherited flags first, then the command's own, each sorted by name.
fn ordered(flags: &Flags) -> impl Iterator<Item = &Entry> {
    let inherited = flags.entries().iter().filter(|e| e.spec.inherited);
    inherited.chain(flags.entries().iter().filter(|e| !e.spec.inherited))
}

fn required_flag_completions(flags: &Flags, to_complete: &str) -> Vec<String> {
    ordered(flags)
        .filter(|e| e.spec.required && !e.changed)
        .flat_map(|e| flag_name_completions(e, to_complete))
        .collect()
}

fn flag_name_completions(e: &Entry, to_complete: &str) -> Vec<String> {
    if e.spec.hidden {
        return Vec::new();
    }
    let mut out = Vec::new();
    let long = format!("--{}", e.spec.name);
    if long.starts_with(to_complete) {
        out.push(with_desc(&long, &e.spec.usage));
    }
    let short = format!("-{}", e.spec.short);
    if !e.spec.short.is_empty() && short.starts_with(to_complete) {
        out.push(with_desc(&short, &e.spec.usage));
    }
    out
}

fn help_topics(tree: Tree, args: &[String], to_complete: &str) -> Vec<String> {
    let (found, checked) = tree.find(args);
    if checked.is_err() {
        return Vec::new();
    }
    tree.children(found.cmd)
        .filter(|sub| tree.is_available(sub) || is_root_help(found.cmd, sub))
        .filter(|sub| name(sub).starts_with(to_complete))
        .map(|sub| with_desc(name(sub), sub.short))
        .collect()
}

/// cobra's checkIfFlagCompletion: whether the word being completed is a
/// flag value, and the arguments left once that flag is set aside.
fn check_flag_completion(
    cmd: &'static Command,
    flags: &Flags,
    args: Vec<String>,
    last: &str,
) -> (Option<&'static str>, Vec<String>, String, Option<String>) {
    if kind(cmd).disable_flag_parsing() {
        return (None, args, last.to_string(), None);
    }
    let mut flag_name = String::new();
    let mut trimmed = args.clone();
    let mut last_arg = last.to_string();
    let mut with_equal = false;
    if last.starts_with('-') {
        let Some(index) = last.find('=') else {
            return (None, args, last_arg, None);
        };
        flag_name = if last[..index].starts_with("--") {
            last[2..index].to_string()
        } else {
            String::from_utf8_lossy(&last.as_bytes()[index - 1..index]).into_owned()
        };
        last_arg = last[index + 1..].to_string();
        with_equal = true;
    }
    if flag_name.is_empty()
        && let Some(prev) = args.last()
        && is_flag_arg(prev)
        && !prev.contains('=')
    {
        flag_name = if let Some(long) = prev.strip_prefix("--") {
            long.to_string()
        } else {
            String::from_utf8_lossy(&prev.as_bytes()[prev.len() - 1..]).into_owned()
        };
        trimmed.pop();
    }
    if flag_name.is_empty() {
        return (None, trimmed, last_arg, None);
    }
    let Some(entry) = find_flag(flags, &flag_name) else {
        let e = format!(
            "Subcommand '{}' does not support flag '{flag_name}'",
            name(cmd)
        );
        return (None, args, last.to_string(), Some(e));
    };
    if !with_equal && !entry.spec.no_opt_default.is_empty() {
        return (None, args, last_arg, None);
    }
    (Some(entry.spec.name), trimmed, last_arg, None)
}

fn find_flag<'f>(flags: &'f Flags, name: &str) -> Option<&'f Entry> {
    match name.as_bytes() {
        [c] => flags.lookup_short(*c),
        _ => flags.lookup(name),
    }
}

fn is_flag_arg(arg: &str) -> bool {
    let b = arg.as_bytes();
    (b.len() >= 3 && b.starts_with(b"--")) || (b.len() >= 2 && b[0] == b'-' && b[1] != b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete(args: &[&str]) -> (String, String) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        run(&args, "__complete", &mut out, &mut err);
        (
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn scripts_name_the_request_command_once() {
        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::Powershell] {
            assert_eq!(script(shell, true).matches("__complete").count(), 1);
            assert_eq!(script(shell, false).matches("__completeNoDesc").count(), 1);
        }
    }

    #[test]
    fn completes_subcommands() {
        assert_eq!(
            complete(&["video", "l"]),
            (
                "list\tList videos\n:4\n".into(),
                "Completion ended with directive: ShellCompDirectiveNoFileComp\n".into()
            )
        );
    }

    #[test]
    fn completes_required_flags_before_others() {
        assert_eq!(
            complete(&["video", "delete", ""]).0,
            "--ids\tIDs of the videos to delete\n-i\tIDs of the videos to delete\n:0\n"
        );
    }

    #[test]
    fn reports_flag_errors_on_stderr() {
        assert_eq!(
            complete(&["video", "list", "--bad", ""]),
            (
                ":0\n".into(),
                "[Debug] [Error] Error while parsing flags from args [--bad]: unknown flag: --bad\nCompletion ended with directive: ShellCompDirectiveDefault\n".into()
            )
        );
    }
}
