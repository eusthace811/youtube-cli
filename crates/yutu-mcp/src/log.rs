//! A private stand-in for the slog-compatible logger that yutu-auth will
//! provide: `time=... level=INFO msg="..." key=value` lines on stderr at
//! the `YUTU_LOG_LEVEL` threshold. Call sites use only [`info`], [`error`],
//! [`raw`] and [`enabled`], so swapping the backend is a one-file change.

use std::io::Write;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    fn name(self) -> &'static str {
        match self {
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }
}

fn threshold() -> Level {
    static LEVEL: OnceLock<Level> = OnceLock::new();
    *LEVEL.get_or_init(|| match std::env::var("YUTU_LOG_LEVEL").as_deref() {
        Ok("DEBUG" | "debug") => Level::Debug,
        Ok("WARN" | "warn") => Level::Warn,
        Ok("ERROR" | "error") => Level::Error,
        _ => Level::Info,
    })
}

pub fn enabled(level: Level) -> bool {
    level >= threshold()
}

pub fn info(msg: &str, attrs: &[(&str, &str)]) {
    log(Level::Info, msg, attrs);
}

pub fn error(msg: &str, attrs: &[(&str, &str)]) {
    log(Level::Error, msg, attrs);
}

/// Writes a raw line, as Go's `mcp.LoggingTransport` does for frames.
pub fn raw(line: &str) {
    let mut err = std::io::stderr().lock();
    let _ = writeln!(err, "{line}");
}

fn log(level: Level, msg: &str, attrs: &[(&str, &str)]) {
    if !enabled(level) {
        return;
    }
    let mut line = format!("time={} level={} msg={}", now(), level.name(), text(msg));
    for (k, v) in attrs {
        line.push(' ');
        line.push_str(k);
        line.push('=');
        line.push_str(&text(v));
    }
    raw(&line);
}

/// slog's TextHandler quoting: strings that are empty or hold spaces, `=`,
/// quotes or unprintable runes are written as Go string literals.
fn text(s: &str) -> String {
    let plain = !s.is_empty()
        && s.chars()
            .all(|c| c != ' ' && c != '=' && c != '"' && !c.is_control());
    if plain { s.to_string() } else { go_quote(s) }
}

/// Go's `strconv.Quote` for the characters log values and `%q` meet.
pub fn go_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{7}' => out.push_str("\\a"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\u{b}' => out.push_str("\\v"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// RFC 3339 with milliseconds in UTC, which is what Go prints under TZ=UTC.
fn now() -> String {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, day) = civil(days);
    format!(
        "{y:04}-{m:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60,
        d.subsec_millis()
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_like_slog() {
        assert_eq!(text("starting MCP server"), "\"starting MCP server\"");
        assert_eq!(text("stdio"), "stdio");
        assert_eq!(text(""), "\"\"");
        assert_eq!(text("stdio, http"), "\"stdio, http\"");
        assert_eq!(text("a\nb"), "\"a\\nb\"");
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_744), (2026, 10, 18));
    }
}
