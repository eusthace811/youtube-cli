//! ADK's `full` launcher argument handling: `--args` split like Go's
//! `strings.Fields`, routed by its first word, and console flags parsed with
//! the Go `flag` package's rules and messages.

use std::io::Write;

/// What `yutu agent` prints for an empty `--args` or a launch failure:
/// the full launcher's `CommandLineSyntax()` plus Println's newline.
pub const SYNTAX: &str = include_str!("../assets/launcher-syntax.txt");

const CONSOLE_DEFAULTS: &str = "  -otel_to_cloud
    \tEnables/disables OpenTelemetry export to GCP: telemetry.googleapis.com. See adk-go/telemetry package for details about supported options, credentials and environment variables.
  -shutdown-timeout duration
    \tConsole shutdown timeout (i.e. '10s', '2m' - see time.ParseDuration for details) - for waiting for active requests to finish during shutdown (default 2s)
  -streaming_mode string
    \tdefines streaming mode (none|sse)
";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Streaming {
    None,
    Sse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleConfig {
    /// `None` picks SSE when stdout is a terminal, as ADK does.
    pub streaming: Option<Streaming>,
    pub otel_to_cloud: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    Console(ConsoleConfig),
    /// `web` and its sublaunchers, which land in R10. Holds the arguments
    /// after the keyword.
    Web(Vec<String>),
}

/// Go's `strings.Fields`.
pub fn fields(s: &str) -> Vec<String> {
    s.split(char::is_whitespace)
        .filter(|f| !f.is_empty())
        .map(String::from)
        .collect()
}

/// The universal launcher's parse. Flag errors print the Go `flag` package's
/// message and usage to `stderr`; the returned error is what ADK returns.
pub fn parse(args: &[String], stderr: &mut dyn Write) -> Result<Launch, String> {
    let rest = match args.first().map(String::as_str) {
        Some("web") => return Ok(Launch::Web(args[1..].to_vec())),
        Some("console") => &args[1..],
        _ => args,
    };
    let (config, remaining) =
        parse_console(rest, stderr).map_err(|e| format!("failed to parse flags: {e}"))?;
    let streaming = match config.streaming_mode.as_str() {
        "" => None,
        "none" => Some(Streaming::None),
        "sse" => Some(Streaming::Sse),
        other => {
            return Err(format!(
                "invalid streaming_mode: {other}. Should be (none|sse)"
            ));
        }
    };
    if !remaining.is_empty() {
        return Err(format!(
            "cannot parse following arguments: [{}]",
            remaining.join(" ")
        ));
    }
    Ok(Launch::Console(ConsoleConfig {
        streaming,
        otel_to_cloud: config.otel_to_cloud,
    }))
}

#[derive(Default)]
struct RawConsole {
    streaming_mode: String,
    otel_to_cloud: bool,
}

#[derive(Clone, Copy)]
enum Kind {
    Bool,
    Duration,
    Str,
}

fn lookup(name: &str) -> Option<Kind> {
    match name {
        "otel_to_cloud" => Some(Kind::Bool),
        "shutdown-timeout" => Some(Kind::Duration),
        "streaming_mode" => Some(Kind::Str),
        _ => None,
    }
}

/// `flag.FlagSet.Parse` with `ContinueOnError` for the console flag set.
fn parse_console(
    args: &[String],
    stderr: &mut dyn Write,
) -> Result<(RawConsole, Vec<String>), String> {
    let mut cfg = RawConsole::default();
    let mut i = 0;
    let mut fail = |msg: String, help: bool| {
        if !help {
            let _ = writeln!(stderr, "{msg}");
        }
        let _ = write!(stderr, "Usage of console:\n{CONSOLE_DEFAULTS}");
        msg
    };
    while i < args.len() {
        let s = &args[i];
        if s.len() < 2 || !s.starts_with('-') {
            break;
        }
        let mut name = &s[1..];
        if let Some(n) = name.strip_prefix('-') {
            if n.is_empty() {
                i += 1;
                break;
            }
            name = n;
        }
        if name.is_empty() || name.starts_with('-') || name.starts_with('=') {
            return Err(fail(format!("bad flag syntax: {s}"), false));
        }
        i += 1;
        let (name, value) = match name.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (name, None),
        };
        let Some(kind) = lookup(name) else {
            if name == "help" || name == "h" {
                return Err(fail("flag: help requested".into(), true));
            }
            return Err(fail(
                format!("flag provided but not defined: -{name}"),
                false,
            ));
        };
        match kind {
            Kind::Bool => {
                let v = value.as_deref().unwrap_or("true");
                match parse_bool(v) {
                    Some(b) => cfg.otel_to_cloud = b,
                    None => {
                        return Err(fail(
                            format!(
                                "invalid boolean value {} for -{name}: parse error",
                                yutu_auth::log::quote(v)
                            ),
                            false,
                        ));
                    }
                }
            }
            Kind::Duration | Kind::Str => {
                let v = match value {
                    Some(v) => v,
                    None if i < args.len() => {
                        i += 1;
                        args[i - 1].clone()
                    }
                    None => return Err(fail(format!("flag needs an argument: -{name}"), false)),
                };
                if matches!(kind, Kind::Duration) {
                    if !valid_duration(&v) {
                        return Err(fail(
                            format!(
                                "invalid value {} for flag -{name}: parse error",
                                yutu_auth::log::quote(&v)
                            ),
                            false,
                        ));
                    }
                } else {
                    cfg.streaming_mode = v;
                }
            }
        }
    }
    Ok((cfg, args[i..].to_vec()))
}

/// Go's `strconv.ParseBool`.
fn parse_bool(s: &str) -> Option<bool> {
    match s {
        "1" | "t" | "T" | "true" | "TRUE" | "True" => Some(true),
        "0" | "f" | "F" | "false" | "FALSE" | "False" => Some(false),
        _ => None,
    }
}

/// Whether Go's `time.ParseDuration` accepts `s`.
fn valid_duration(s: &str) -> bool {
    let s = s.strip_prefix(['-', '+']).unwrap_or(s);
    if s == "0" {
        return true;
    }
    if s.is_empty() {
        return false;
    }
    let mut rest = s;
    while !rest.is_empty() {
        let digits = rest
            .find(|c: char| !c.is_ascii_digit() && c != '.')
            .unwrap_or(rest.len());
        let number = &rest[..digits];
        if number.is_empty() || number == "." || number.matches('.').count() > 1 {
            return false;
        }
        rest = &rest[digits..];
        let unit = rest
            .find(|c: char| c.is_ascii_digit() || c == '.')
            .unwrap_or(rest.len());
        if !matches!(
            &rest[..unit],
            "ns" | "us" | "µs" | "μs" | "ms" | "s" | "m" | "h"
        ) {
            return false;
        }
        rest = &rest[unit..];
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &str) -> (Result<Launch, String>, String) {
        let mut err = Vec::new();
        let r = parse(&fields(args), &mut err);
        (r, String::from_utf8(err).unwrap())
    }

    fn console(streaming: Option<Streaming>) -> Result<Launch, String> {
        Ok(Launch::Console(ConsoleConfig {
            streaming,
            otel_to_cloud: false,
        }))
    }

    #[test]
    fn routes_like_the_universal_launcher() {
        assert_eq!(run("console").0, console(None));
        assert_eq!(run("").0, console(None));
        assert_eq!(
            run("  -streaming_mode=sse ").0,
            console(Some(Streaming::Sse))
        );
        assert_eq!(
            run("console --streaming_mode none").0,
            console(Some(Streaming::None))
        );
        assert_eq!(run("console -shutdown-timeout 1m30s").0, console(None));
        assert_eq!(run("web api").0, Ok(Launch::Web(vec!["api".into()])));
        assert_eq!(
            run("foo bar").0,
            Err("cannot parse following arguments: [foo bar]".into())
        );
        assert_eq!(
            run("console -streaming_mode=foo").0,
            Err("invalid streaming_mode: foo. Should be (none|sse)".into())
        );
    }

    #[test]
    fn flag_errors_print_go_usage() {
        let usage = format!("Usage of console:\n{CONSOLE_DEFAULTS}");
        let (r, err) = run("console -x");
        assert_eq!(
            r,
            Err("failed to parse flags: flag provided but not defined: -x".into())
        );
        assert_eq!(err, format!("flag provided but not defined: -x\n{usage}"));
        let (r, err) = run("console -h");
        assert_eq!(r, Err("failed to parse flags: flag: help requested".into()));
        assert_eq!(err, usage);
        let (r, _) = run("console -shutdown-timeout x");
        assert_eq!(
            r,
            Err(r#"failed to parse flags: invalid value "x" for flag -shutdown-timeout: parse error"#.into())
        );
        let (r, _) = run("-streaming_mode");
        assert_eq!(
            r,
            Err("failed to parse flags: flag needs an argument: -streaming_mode".into())
        );
        let (r, _) = run("console -otel_to_cloud=maybe");
        assert_eq!(
            r,
            Err(r#"failed to parse flags: invalid boolean value "maybe" for -otel_to_cloud: parse error"#.into())
        );
    }

    #[test]
    fn syntax_carries_the_console_flags() {
        assert!(SYNTAX.contains(&format!("  console\n{CONSOLE_DEFAULTS}\n  web\n")));
    }
}
