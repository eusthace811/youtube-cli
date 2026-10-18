//! A cobra/pflag-compatible command engine driven by the registry. It
//! reproduces cobra v1.10.2 and pflag v1.0.10 byte for byte: parsing,
//! help and usage text, error messages, suggestions and completion.

mod complete;
mod flags;
mod goconv;
mod help;
mod isprint;
mod tree;

use std::io::Write;

pub use flags::Flags;

use crate::registry::Command;
use tree::{ArgsRule, Kind, Tree, command_path, kind};

/// A runnable leaf command with its parsed flags and positional arguments.
pub struct Invocation {
    pub command: &'static Command,
    pub flags: Flags,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CmdError {
    /// Printed as `Error: <msg>` and the usage hint, exit 1.
    Message(String),
    /// Already reported; exit silently with this code.
    Exit(i32),
}

/// The operations behind leaf commands. The engine handles help,
/// completion and parent commands itself.
pub trait App {
    /// cobra's PreRunE: runs before required flags are checked.
    fn pre_run(&self, inv: &Invocation) -> Result<(), CmdError>;
    /// cobra's RunE.
    fn run(&self, inv: &Invocation) -> Result<(), CmdError>;
}

/// Runs the command line `args` (without the program name) and returns the
/// process exit code.
pub fn execute(args: Vec<String>, app: &dyn App) -> i32 {
    let (stdout, stderr) = (std::io::stdout(), std::io::stderr());
    let (mut out, mut err) = (stdout.lock(), stderr.lock());
    let code = execute_with(args, app, &mut out, &mut err);
    let _ = out.flush();
    code
}

/// [`execute`] with the engine's own output sent to `out` and `err`.
pub fn execute_with(
    args: Vec<String>,
    app: &dyn App,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    // cobra attaches __complete only when it is the command being invoked.
    let mut tree = Tree { complete: true };
    let (mut found, mut checked) = tree.find(&args);
    if checked.is_err() || !tree::is(found.cmd, &tree::COMPLETE) {
        tree.complete = false;
        (found, checked) = tree.find(&args);
    }
    let path = command_path(found.cmd);
    if let Err(e) = checked {
        let _ = write!(
            err,
            "Error: {e}\nRun '{path} --help' for usage.\nRun '{path} -h' to see usage.\n"
        );
        return 1;
    }
    match run(tree, found, app, out, err) {
        Ok(()) => 0,
        Err(CmdError::Message(msg)) => {
            let _ = write!(err, "Error: {msg}\nRun '{path} -h' to see usage.\n");
            1
        }
        Err(CmdError::Exit(code)) => code,
    }
}

/// cobra's Command.execute for the found command.
fn run(
    tree: Tree,
    found: tree::Found,
    app: &dyn App,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), CmdError> {
    let cmd = found.cmd;
    let k = kind(cmd);
    let mut flags = Flags::for_command(cmd, true);
    let args = if k.disable_flag_parsing() {
        found.args
    } else {
        flags
            .parse(&found.args)
            .map_err(|e| CmdError::Message(e.to_string()))?
    };
    let help_text = || help::help(tree, cmd, true);
    if flags.bool("help") || !cmd.runnable {
        let _ = out.write_all(help_text().as_bytes());
        return Ok(());
    }
    validate_args(k.args_rule(), cmd, &args)?;
    let inv = Invocation {
        command: cmd,
        flags,
        args,
    };
    if k == Kind::Leaf {
        app.pre_run(&inv)?;
    }
    let missing = inv.flags.missing_required();
    if !missing.is_empty() {
        return Err(CmdError::Message(format!(
            "required flag(s) \"{}\" not set",
            missing.join("\", \"")
        )));
    }
    match k {
        Kind::Leaf => app.run(&inv)?,
        Kind::Group | Kind::CompletionGroup => {
            let _ = out.write_all(help_text().as_bytes());
        }
        Kind::Help => help_topic(tree, &inv.args, out, err),
        Kind::Script(shell) => {
            let _ = out
                .write_all(complete::script(shell, !inv.flags.bool("no-descriptions")).as_bytes());
        }
        Kind::Complete => complete::run(&inv.args, &found.called_as, out, err),
    }
    Ok(())
}

fn validate_args(rule: ArgsRule, cmd: &'static Command, args: &[String]) -> Result<(), CmdError> {
    match rule {
        ArgsRule::Legacy => Ok(()),
        ArgsRule::NoArgs if !args.is_empty() => Err(CmdError::Message(format!(
            "unknown command {} for {}",
            goconv::quote(&args[0]),
            goconv::quote(&command_path(cmd))
        ))),
        ArgsRule::MinimumOne if args.is_empty() => Err(CmdError::Message(format!(
            "requires at least 1 arg(s), only received {}",
            args.len()
        ))),
        _ => Ok(()),
    }
}

/// The `help [command]` built-in.
fn help_topic(tree: Tree, args: &[String], out: &mut dyn Write, err: &mut dyn Write) {
    let (found, checked) = tree.find(args);
    if checked.is_ok() {
        let _ = out.write_all(help::help(tree, found.cmd, true).as_bytes());
        return;
    }
    let topics: Vec<String> = args.iter().map(|a| goconv::backquote(a)).collect();
    let _ = write!(
        err,
        "Unknown help topic [{}]\n{}",
        topics.join(" "),
        help::usage(tree, tree::root(), false)
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Recorder {
        calls: RefCell<Vec<String>>,
        pre_run: Option<CmdError>,
    }

    impl App for Recorder {
        fn pre_run(&self, inv: &Invocation) -> Result<(), CmdError> {
            self.calls
                .borrow_mut()
                .push(format!("pre_run {}", inv.command.path));
            self.pre_run.clone().map_or(Ok(()), Err)
        }

        fn run(&self, inv: &Invocation) -> Result<(), CmdError> {
            self.calls
                .borrow_mut()
                .push(format!("run {} {:?}", inv.command.path, inv.args));
            Ok(())
        }
    }

    fn exec(app: &Recorder, args: &[&str]) -> (i32, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = execute_with(
            args.iter().map(|s| s.to_string()).collect(),
            app,
            &mut out,
            &mut err,
        );
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn pre_run_runs_before_the_required_flag_check() {
        let gate = Recorder {
            pre_run: Some(CmdError::Message("operation not confirmed".into())),
            ..Default::default()
        };
        let (code, out, err) = exec(&gate, &["video", "delete"]);
        assert_eq!((code, out.as_str()), (1, ""));
        assert_eq!(
            err,
            "Error: operation not confirmed\nRun 'yutu video delete -h' to see usage.\n"
        );

        let open = Recorder::default();
        let (code, _, err) = exec(&open, &["video", "delete", "--yes"]);
        assert_eq!(code, 1);
        assert_eq!(
            err,
            "Error: required flag(s) \"ids\" not set\nRun 'yutu video delete -h' to see usage.\n"
        );
        assert_eq!(*open.calls.borrow(), ["pre_run video delete"]);
    }

    #[test]
    fn leaves_reach_the_app_with_positionals() {
        let app = Recorder::default();
        let (code, out, err) = exec(&app, &["version", "extra"]);
        assert_eq!((code, out.as_str(), err.as_str()), (0, "", ""));
        assert_eq!(
            *app.calls.borrow(),
            ["pre_run version", "run version [\"extra\"]"]
        );
    }

    #[test]
    fn exit_errors_are_silent() {
        struct Fails;
        impl App for Fails {
            fn pre_run(&self, _: &Invocation) -> Result<(), CmdError> {
                Ok(())
            }
            fn run(&self, _: &Invocation) -> Result<(), CmdError> {
                Err(CmdError::Exit(3))
            }
        }
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(
            execute_with(vec!["auth".into()], &Fails, &mut out, &mut err),
            3
        );
        assert!(out.is_empty() && err.is_empty());
    }

    #[test]
    fn unknown_commands_print_cobra_and_yutu_hints() {
        let (code, out, err) = exec(&Recorder::default(), &["nope"]);
        assert_eq!((code, out.as_str()), (1, ""));
        assert_eq!(
            err,
            "Error: unknown command \"nope\" for \"yutu\"\nRun 'yutu --help' for usage.\nRun 'yutu -h' to see usage.\n"
        );
    }

    #[test]
    fn help_flag_and_parent_commands_print_help_without_the_app() {
        let app = Recorder::default();
        let (code, out, _) = exec(&app, &["video", "list", "-h"]);
        assert_eq!(code, 0);
        assert!(out.starts_with(
            "List videos. Use this tool to list videos.\n\nUsage:\n  yutu video list [flags]\n"
        ));
        let (code, out, _) = exec(&app, &["video", "nope"]);
        assert_eq!(code, 0);
        assert!(out.starts_with("Manage YouTube videos."));
        assert!(app.calls.borrow().is_empty());
    }

    #[test]
    fn help_command_reports_unknown_topics_on_stderr() {
        let (code, out, err) = exec(&Recorder::default(), &["help", "nope", "b`c"]);
        assert_eq!((code, out.as_str()), (0, ""));
        assert!(err.starts_with(
            "Unknown help topic [`nope` \"b`c\"]\nUsage:\n  yutu\n  yutu [command]\n\n"
        ));
    }

    #[test]
    fn completion_scripts_reject_arguments() {
        let (code, _, err) = exec(&Recorder::default(), &["completion", "bash", "extra"]);
        assert_eq!(code, 1);
        assert_eq!(
            err,
            "Error: unknown command \"extra\" for \"yutu completion bash\"\nRun 'yutu completion bash -h' to see usage.\n"
        );
    }

    #[test]
    fn complete_needs_an_argument() {
        let (code, _, err) = exec(&Recorder::default(), &["__complete"]);
        assert_eq!(code, 1);
        assert_eq!(
            err,
            "Error: requires at least 1 arg(s), only received 0\nRun 'yutu __complete -h' to see usage.\n"
        );
    }

    #[test]
    fn complete_without_descriptions_by_alias() {
        let (code, out, _) = exec(&Recorder::default(), &["__completeNoDesc", "vi"]);
        assert_eq!(code, 0);
        assert_eq!(out, "video\nvideoAbuseReportReason\nvideoCategory\n:4\n");
    }
}
