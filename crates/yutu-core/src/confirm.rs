//! The CLI confirmation gate, as Go's `utils.ConfirmPreRun`: `--yes` skips
//! it; without a terminal on stdin (or with `CI` set) the message goes to
//! stderr and the command fails; on a terminal only `y` or `Y` accepts.

use std::io::{BufRead, IsTerminal, Write};

pub const NOT_CONFIRMED: &str = "operation not confirmed (pass confirm: true or rerun with --yes)";

/// Go's `utils.IsInteractive` for a file descriptor.
pub fn interactive(is_terminal: bool) -> bool {
    std::env::var("CI").unwrap_or_default().is_empty() && is_terminal
}

pub fn confirm(yes: bool, message: &str) -> Result<(), String> {
    if yes {
        return Ok(());
    }
    let mut stderr = std::io::stderr();
    let stdin = std::io::stdin();
    if !interactive(stdin.is_terminal()) {
        let _ = writeln!(stderr, "{message}");
        return Err(NOT_CONFIRMED.into());
    }
    let _ = write!(stderr, "{message} [y/N] ");
    let _ = stderr.flush();
    let mut line = String::new();
    match stdin.lock().read_line(&mut line) {
        Ok(n) if n > 0 => {}
        _ => return Err(NOT_CONFIRMED.into()),
    }
    match line.trim() {
        "y" | "Y" => Ok(()),
        _ => Err(NOT_CONFIRMED.into()),
    }
}
