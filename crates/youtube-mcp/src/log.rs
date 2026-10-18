//! The MCP server's logging, through youtube-auth's slog-compatible logger.
//! Call sites pass string attributes; [`raw`] writes JSON-RPC frames.

use std::io::Write;

pub use youtube_auth::log::{Level, enabled};

fn attrs<'a>(pairs: &'a [(&'a str, &'a str)]) -> Vec<youtube_auth::log::Attr<'a>> {
    pairs
        .iter()
        .map(|(k, v)| (*k, v as &dyn std::fmt::Display))
        .collect()
}

pub fn info(msg: &str, pairs: &[(&str, &str)]) {
    youtube_auth::log::info(msg, &attrs(pairs));
}

pub fn error(msg: &str, pairs: &[(&str, &str)]) {
    youtube_auth::log::error(msg, &attrs(pairs));
}

/// Writes a raw line, as Go's `mcp.LoggingTransport` does for frames.
pub fn raw(line: &str) {
    let mut err = std::io::stderr().lock();
    let _ = writeln!(err, "{line}");
}

/// Go's `strconv.Quote`, used in go-sdk error texts.
pub fn go_quote(s: &str) -> String {
    youtube_auth::log::quote(s)
}
