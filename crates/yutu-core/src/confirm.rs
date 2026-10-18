//! The CLI confirmation gate, as Go's `utils.ConfirmPreRun`: `--yes` skips
//! it; without a terminal on stdin (or with `CI` set) the message goes to
//! stderr and the command fails; on a terminal only `y` or `Y` accepts.

use std::io::{BufRead, IsTerminal, Write};

pub const NOT_CONFIRMED: &str = "operation not confirmed (pass confirm: true or rerun with --yes)";

/// A confirmation message safe to show a person: control characters, which
/// could redraw the terminal around the question, and the invisible format
/// characters that reorder or hide text (bidi controls, zero-width marks)
/// are escaped. Messages are built from tool arguments a model or MCP
/// client chose.
pub fn printable(message: &str) -> String {
    let invisible = |c: char| matches!(c, '\u{ad}' | '\u{61c}' | '\u{180e}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2069}' | '\u{feff}');
    message
        .chars()
        .map(|c| {
            if c.is_control() || invisible(c) {
                format!("\\u{{{:x}}}", u32::from(c))
            } else {
                c.to_string()
            }
        })
        .collect()
}

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

#[cfg(test)]
mod tests {
    #[test]
    fn control_characters_are_shown_not_obeyed() {
        assert_eq!(
            super::printable("Delete video(s): REAL,\u{1b}[2K\r\nList video(s): x"),
            "Delete video(s): REAL,\\u{1b}[2K\\u{d}\\u{a}List video(s): x"
        );
        assert_eq!(
            super::printable("Delete video(s): v1"),
            "Delete video(s): v1"
        );
        assert_eq!(
            super::printable("Delete: \u{202e}1v\u{200b}"),
            "Delete: \\u{202e}1v\\u{200b}"
        );
    }
}
