//! The process logger, byte-compatible with Go's `slog.NewTextHandler(os.Stderr)`
//! at the level `YUTU_LOG_LEVEL` selects.

use crate::isprint::{IS_NOT_PRINT16, IS_NOT_PRINT32, IS_PRINT16, IS_PRINT32};
use std::cell::RefCell;
use std::fmt::{Display, Write as _};
use std::io::Write as _;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }
}

pub type Attr<'a> = (&'a str, &'a dyn Display);

/// Go accepts only these spellings; anything else keeps the INFO default.
pub fn parse_level(value: Option<&str>) -> Level {
    match value {
        Some("DEBUG" | "debug") => Level::Debug,
        Some("WARN" | "warn") => Level::Warn,
        Some("ERROR" | "error") => Level::Error,
        _ => Level::Info,
    }
}

fn min_level() -> Level {
    static LEVEL: OnceLock<Level> = OnceLock::new();
    *LEVEL.get_or_init(|| parse_level(std::env::var("YUTU_LOG_LEVEL").ok().as_deref()))
}

pub fn enabled(level: Level) -> bool {
    level >= min_level()
}

pub fn debug(msg: &str, attrs: &[Attr<'_>]) {
    log(Level::Debug, msg, attrs);
}

pub fn info(msg: &str, attrs: &[Attr<'_>]) {
    log(Level::Info, msg, attrs);
}

pub fn warn(msg: &str, attrs: &[Attr<'_>]) {
    log(Level::Warn, msg, attrs);
}

pub fn error(msg: &str, attrs: &[Attr<'_>]) {
    log(Level::Error, msg, attrs);
}

thread_local! {
    static CAPTURE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Runs `f` with this thread's log lines collected instead of written to
/// stderr, so tests can assert on exactly what would have been logged.
pub fn capture<R>(f: impl FnOnce() -> R) -> (R, String) {
    let previous = CAPTURE.with(|c| c.replace(Some(String::new())));
    let result = f();
    let captured = CAPTURE.with(|c| c.replace(previous)).unwrap_or_default();
    (result, captured)
}

pub fn log(level: Level, msg: &str, attrs: &[Attr<'_>]) {
    if !enabled(level) {
        return;
    }
    let line = format_line(&chrono::Local::now().fixed_offset(), level, msg, attrs);
    let captured = CAPTURE.with(|c| match c.borrow_mut().as_mut() {
        Some(buf) => {
            buf.push_str(&line);
            true
        }
        None => false,
    });
    if !captured {
        let _ = std::io::stderr().lock().write_all(line.as_bytes());
    }
}

pub fn format_line(
    now: &chrono::DateTime<chrono::FixedOffset>,
    level: Level,
    msg: &str,
    attrs: &[Attr<'_>],
) -> String {
    let mut line = String::with_capacity(64);
    line.push_str("time=");
    push_rfc3339_millis(&mut line, now);
    line.push_str(" level=");
    line.push_str(level.as_str());
    line.push_str(" msg=");
    push_text(&mut line, msg);
    for (key, value) in attrs {
        line.push(' ');
        push_text(&mut line, key);
        line.push('=');
        push_text(&mut line, &value.to_string());
    }
    line.push('\n');
    line
}

fn push_rfc3339_millis(out: &mut String, t: &chrono::DateTime<chrono::FixedOffset>) {
    let _ = write!(out, "{}", t.format("%Y-%m-%dT%H:%M:%S%.3f"));
    push_offset(out, t.offset().local_minus_utc());
}

/// Go's `Z07:00` zone layout.
pub(crate) fn push_offset(out: &mut String, seconds: i32) {
    if seconds == 0 {
        out.push('Z');
        return;
    }
    let sign = if seconds < 0 { '-' } else { '+' };
    let minutes = seconds.unsigned_abs() / 60;
    let _ = write!(out, "{sign}{:02}:{:02}", minutes / 60, minutes % 60);
}

fn push_text(out: &mut String, s: &str) {
    if needs_quoting(s) {
        push_quoted(out, s);
    } else {
        out.push_str(s);
    }
}

fn needs_quoting(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    s.chars().any(|c| {
        if c.is_ascii() {
            c != '\\' && (c == ' ' || c == '=' || c == '"' || (c as u32) < 0x20)
        } else {
            c == char::REPLACEMENT_CHARACTER || c.is_whitespace() || !is_print(c)
        }
    })
}

/// Go's `strconv.Quote`.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    push_quoted(&mut out, s);
    out
}

fn push_quoted(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c if is_print(c) => out.push(c),
            '\x07' => out.push_str("\\a"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x0b' => out.push_str("\\v"),
            c if (c as u32) < 0x20 || c == '\x7f' => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c if (c as u32) < 0x10000 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => {
                let _ = write!(out, "\\U{:08x}", c as u32);
            }
        }
    }
    out.push('"');
}

/// Go's `strconv.IsPrint`, over the tables Go generates from Unicode.
pub fn is_print(c: char) -> bool {
    let r = c as u32;
    if r <= 0xff {
        return (0x20..=0x7e).contains(&r) || ((0xa1..=0xff).contains(&r) && r != 0xad);
    }
    if r < 0x10000 {
        let rr = r as u16;
        let i = IS_PRINT16.partition_point(|&x| x < rr);
        if i >= IS_PRINT16.len() || rr < IS_PRINT16[i & !1] || IS_PRINT16[i | 1] < rr {
            return false;
        }
        return IS_NOT_PRINT16.binary_search(&rr).is_err();
    }
    let i = IS_PRINT32.partition_point(|&x| x < r);
    if i >= IS_PRINT32.len() || r < IS_PRINT32[i & !1] || IS_PRINT32[i | 1] < r {
        return false;
    }
    if r >= 0x20000 {
        return true;
    }
    IS_NOT_PRINT32
        .binary_search(&((r - 0x10000) as u16))
        .is_err()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, TimeZone};

    #[test]
    fn line_matches_go_text_handler() {
        let now = FixedOffset::east_opt(3600)
            .unwrap()
            .with_ymd_and_hms(2026, 10, 18, 16, 0, 57)
            .unwrap()
            + chrono::Duration::microseconds(133_999);
        let line = format_line(
            &now,
            Level::Warn,
            "failed to read token",
            &[
                ("path", &"/tmp/a b/youtube.token.json"),
                ("k=v", &"a=b"),
                ("empty", &""),
                ("uni", &"héllo"),
                ("nbsp", &"a\u{a0}b"),
                ("zw", &"a\u{200b}b"),
                ("q", &"x\"y"),
                ("bs", &"a\\b"),
                ("nl", &"a\nb"),
                ("tab", &"a\tb"),
                ("ctl", &"\x01"),
                ("del", &"\x7f"),
                ("pua", &"\u{e000}"),
                ("un", &"\u{e0080}"),
                ("fffd", &"\u{fffd}"),
                ("emoji", &"🎉"),
                ("n", &42),
                ("d", &"2m0s"),
                ("comb", &"e\u{301}"),
                ("ls", &"a\u{2028}b"),
                ("bell", &"\x07\x0b"),
            ],
        );
        // Captured from Go 1.27.1 slog.NewTextHandler with the same attributes.
        assert_eq!(
            line,
            "time=2026-10-18T16:00:57.133+01:00 level=WARN msg=\"failed to read token\" path=\"/tmp/a b/youtube.token.json\" \"k=v\"=\"a=b\" empty=\"\" uni=héllo nbsp=\"a\\u00a0b\" zw=\"a\\u200bb\" q=\"x\\\"y\" bs=a\\b nl=\"a\\nb\" tab=\"a\\tb\" ctl=\"\\x01\" del=\x7f pua=\"\\ue000\" un=\"\\U000e0080\" fffd=\"\u{fffd}\" emoji=🎉 n=42 d=2m0s comb=e\u{301} ls=\"a\\u2028b\" bell=\"\\a\\v\"\n"
        );
    }

    #[test]
    fn utc_and_negative_offsets() {
        let utc = FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(2026, 1, 2, 3, 4, 5)
            .unwrap();
        assert_eq!(
            format_line(&utc, Level::Error, "", &[]),
            "time=2026-01-02T03:04:05.000Z level=ERROR msg=\"\"\n"
        );
        let west = FixedOffset::west_opt(5 * 3600 + 1800)
            .unwrap()
            .with_ymd_and_hms(2026, 1, 2, 3, 4, 5)
            .unwrap()
            + chrono::Duration::nanoseconds(999_999_999);
        assert_eq!(
            format_line(&west, Level::Debug, "Root directory set", &[("dir", &"/Users/juan")]),
            "time=2026-01-02T03:04:05.999-05:30 level=DEBUG msg=\"Root directory set\" dir=/Users/juan\n"
        );
    }

    #[test]
    fn level_spellings() {
        assert_eq!(parse_level(Some("debug")), Level::Debug);
        assert_eq!(parse_level(Some("WARN")), Level::Warn);
        assert_eq!(parse_level(Some("error")), Level::Error);
        assert_eq!(parse_level(Some("Debug")), Level::Info);
        assert_eq!(parse_level(None), Level::Info);
    }
}
