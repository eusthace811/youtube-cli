//! `xtask egress BINARY...`: fails when a binary's strings name a network
//! host outside the hosts yutu is allowed to reach.

use std::collections::BTreeMap;

const ALLOWED: &[&str] = &[
    "accounts.google.com",
    "oauth2.googleapis.com",
    "www.googleapis.com",
    "youtube.googleapis.com",
    "generativelanguage.googleapis.com",
    "api.openai.com",
    "localhost",
    "127.0.0.1",
];

/// URLs yutu prints but never requests, each with the text that carries
/// it. An occurrence is exempt only when the binary spells out the whole
/// URL and does not extend its path, so other URLs on the same host fail.
const DISPLAY_ONLY: &[(&str, &str)] = &[
    (
        "http://[",
        "the format string for bracketed IPv6 listener URLs",
    ),
    (
        "https://accounts.google.com",
        "the authorization server in the HTTP MCP protected-resource metadata, packed against the next literal",
    ),
    (
        "https://ai.google.dev/gemini-api/docs/api-key",
        "the agent's missing Gemini key error, as Go's genai prints it",
    ),
    (
        "https://google.github.io/adk-docs/a2a/a2a-extension/",
        "the A2A extension identifier ADK puts on the agent card",
    ),
    (
        "https://github.com/eat-pray-ai/yutu",
        "the `yutu version` banner link",
    ),
    (
        "https://github.com/spf13/cobra/issues/1279",
        "a comment in cobra's fish completion script",
    ),
    (
        "https://github.com/spf13/cobra/issues/1508",
        "a comment in cobra's bash completion script",
    ),
    (
        "https://github.com/eat-pray-ai/yutu#prerequisites",
        "the yutu-auth error that points at the README's credential setup",
    ),
    (
        "https://docs.rs/rustls/latest/rustls/#cryptography-providers",
        "rustls's missing-crypto-provider panic message",
    ),
    (
        "https://docs.rs/rustls/latest/rustls/manual/_03_howto/index.html#unexpected-eof",
        "rustls's missing close_notify error message",
    ),
    (
        "https://custom.endpoint.com/",
        "the `yutu agent --help` example of --base-url",
    ),
    (
        "https://api.deepseek.com/",
        "the `yutu agent --help` example of --base-url",
    ),
];

const SCHEMES: &[&str] = &["https", "http", "wss", "ws"];

/// With `--release`, a binary that still carries the parity test hooks
/// (any `YUTU_PARITY_` variable) fails too: they belong only in builds with
/// the `parity-endpoint` feature.
pub fn run(args: &[String]) -> Result<(), String> {
    let release = args.first().is_some_and(|a| a == "--release");
    let args = if release { &args[1..] } else { args };
    if args.is_empty() {
        return Err("usage: xtask egress [--release] BINARY...".to_string());
    }
    if release {
        for path in args {
            let bytes = std::fs::read(path).map_err(|e| format!("egress: {path}: {e}"))?;
            if bytes.windows(12).any(|w| w == b"YUTU_PARITY_") {
                return Err(format!(
                    "egress: {path} carries parity test hooks (YUTU_PARITY_*)"
                ));
            }
        }
        println!("egress: no parity test hooks");
    }
    let mut unexpected: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut displayed: BTreeMap<&str, &str> = BTreeMap::new();
    for path in args {
        let mut bytes = std::fs::read(path).map_err(|e| format!("egress: {path}: {e}"))?;
        if mask_brotli_dictionary(&mut bytes) {
            println!(
                "egress: masked the brotli static dictionary (RFC 7932 Appendix A, {} bytes of word fragments the dev UI's decompressor carries)",
                DICTIONARY.len()
            );
        }
        for found in urls(&bytes) {
            if ALLOWED.contains(&found.host.as_str()) {
                continue;
            }
            if let Some((url, source)) = DISPLAY_ONLY
                .iter()
                .find(|(url, _)| is_display_only(&found.text, url))
            {
                displayed.insert(url, source);
                continue;
            }
            let urls = unexpected.entry(found.host).or_default();
            if !urls.contains(&found.url) {
                urls.push(found.url);
            }
        }
    }
    for (url, source) in &displayed {
        println!("egress: display-only {url} ({source})");
    }
    for (host, urls) in &unexpected {
        println!("egress: unexpected host {host}: {}", urls.join(" "));
    }
    let summary = format!("egress: {} unexpected hosts", unexpected.len());
    if unexpected.is_empty() {
        println!("{summary}");
        Ok(())
    } else {
        Err(summary)
    }
}

const DICTIONARY: &[u8] = &brotli_decompressor::dictionary::kBrotliDictionary;

/// Blanks the brotli static dictionary out of `bytes`: it is a fixed table
/// of English and web text fragments, among them "http://" next to words,
/// not addresses the program contacts. Only an exact copy is masked.
fn mask_brotli_dictionary(bytes: &mut [u8]) -> bool {
    let Some(at) = bytes
        .windows(DICTIONARY.len())
        .position(|w| w == DICTIONARY)
    else {
        return false;
    };
    bytes[at..at + DICTIONARY.len()].fill(0);
    true
}

/// Packed literals can follow the URL directly, so any byte may follow it
/// except one that continues its path, query or host.
fn is_display_only(text: &str, url: &str) -> bool {
    text.strip_prefix(url)
        .is_some_and(|after| !after.starts_with(['/', '?', '#', '.', ':', '@']))
}

struct Found {
    host: String,
    /// The URL as far as URL characters reach.
    url: String,
    /// Everything from the scheme to the end of the printable run.
    text: String,
}

/// Every http, https, ws or wss URL with a host in the binary's printable
/// strings.
fn urls(bytes: &[u8]) -> Vec<Found> {
    let mut out = Vec::new();
    for run in bytes.split(|b| !(0x20..0x7f).contains(b)) {
        let mut from = 0;
        while let Some(pos) = run[from..].windows(3).position(|w| w == b"://") {
            let sep = from + pos;
            from = sep + 3;
            let letters = run[..sep]
                .iter()
                .rev()
                .take_while(|b| b.is_ascii_alphabetic())
                .count();
            let candidate = run[sep - letters..sep].to_ascii_lowercase();
            // Rust packs string literals without separators, so another
            // literal's letters can run into the scheme.
            let Some(scheme) = SCHEMES.iter().find(|s| candidate.ends_with(s.as_bytes())) else {
                continue;
            };
            let rest = &run[from..];
            let authority_len = rest
                .iter()
                .position(|b| !(b.is_ascii_alphanumeric() || b"-._~@[]:%".contains(b)))
                .unwrap_or(rest.len());
            let authority = &rest[..authority_len];
            let after_user = authority
                .iter()
                .rposition(|&b| b == b'@')
                .map_or(authority, |i| &authority[i + 1..]);
            let host = if after_user.first() == Some(&b'[') {
                after_user
                    .iter()
                    .position(|&b| b == b']')
                    .map_or(after_user, |i| &after_user[..=i])
            } else {
                after_user.split(|&b| b == b':').next().unwrap_or_default()
            };
            let host = String::from_utf8_lossy(host)
                .trim_end_matches('.')
                .to_ascii_lowercase();
            if host.is_empty() {
                continue;
            }
            let text = &run[sep - scheme.len()..];
            let url_len = text
                .iter()
                .position(|b| !(b.is_ascii_alphanumeric() || b"-._~:/?#[]@!$&*+,;=%".contains(b)))
                .unwrap_or(text.len());
            out.push(Found {
                host,
                url: String::from_utf8_lossy(&text[..url_len]).into_owned(),
                text: String::from_utf8_lossy(text).into_owned(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosts(text: &[u8]) -> Vec<String> {
        urls(text).into_iter().map(|f| f.host).collect()
    }

    #[test]
    fn extracts_hosts_from_urls() {
        assert_eq!(
            hosts(b"\0\x01see https://Example.COM/path?q=1\0"),
            ["example.com"]
        );
        assert_eq!(
            hosts(b"usagehttps://a.example:8443 wss://user@b.example/x"),
            ["a.example", "b.example"]
        );
        assert_eq!(
            hosts(b"http://localhost:8216 http://127.0.0.1/ ws://[::1]:9/"),
            ["localhost", "127.0.0.1", "[::1]"]
        );
        assert!(hosts(b"ftp://files.example http://<host>:<port> file:///etc").is_empty());
    }

    #[test]
    fn keeps_the_url_and_the_packed_text_after_it() {
        let found = urls(b"\x01Star https://github.com/eat-pray-ai/yutuv0.11.0Gopher\x00");
        assert_eq!(
            found[0].url,
            "https://github.com/eat-pray-ai/yutuv0.11.0Gopher"
        );
        assert!(is_display_only(
            &found[0].text,
            "https://github.com/eat-pray-ai/yutu"
        ));
    }

    #[test]
    fn display_only_urls_do_not_cover_longer_paths() {
        assert!(!is_display_only(
            "https://github.com/eat-pray-ai/yutu/releases/latest",
            "https://github.com/eat-pray-ai/yutu"
        ));
        assert!(!is_display_only(
            "https://github.com/eat-pray-ai/yutu.evil",
            "https://github.com/eat-pray-ai/yutu"
        ));
        assert!(!is_display_only(
            "https://github.com/other",
            "https://github.com/eat-pray-ai/yutu"
        ));
    }
}
