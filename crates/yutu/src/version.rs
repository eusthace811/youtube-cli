//! `yutu version`: Go yutu's banner, with termlink's hyperlink heuristics.

const VERSION: &str = env!("YUTU_VERSION");
const COMMIT_DATE: &str = env!("YUTU_COMMIT_DATE");
const BUILDER: &str = env!("YUTU_BUILDER");
const REPO: &str = "Github/eat-pray-ai/yutu";
const REPO_URL: &str = "https://github.com/eat-pray-ai/yutu";

/// GOOS for this build.
fn go_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        os => os,
    }
}

/// GOARCH for this build.
fn go_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        "x86" => "386",
        arch => arch,
    }
}

pub fn banner(env: &dyn Fn(&str) -> Option<String>) -> String {
    format!(
        "  _(\\_/)_     yutu {VERSION} {}/{}\n │   ▶   │    CLI, MCP server, and AI agent to automate YouTube workflows\n │ • x • │    📦 build: {BUILDER}-{COMMIT_DATE}\n ╰─/───\\─╯    🌟 Star:  {}\n",
        go_os(),
        go_arch(),
        link(REPO, REPO_URL, env)
    )
}

pub fn process_env(name: &str) -> Option<String> {
    std::env::var_os(name).map(|v| v.to_string_lossy().into_owned())
}

/// termlink.Link: an OSC 8 hyperlink when the terminal supports one.
fn link(text: &str, url: &str, env: &dyn Fn(&str) -> Option<String>) -> String {
    if supports_hyperlinks(env) {
        format!("\x1b]8;;{url}\x07{text}\x1b]8;;\x07\x1b[0m")
    } else {
        format!("{text} ({url})\x1b[0m")
    }
}

fn supports_hyperlinks(env: &dyn Fn(&str) -> Option<String>) -> bool {
    let matches =
        |name: &str, values: &[&str]| env(name).is_some_and(|v| values.contains(&v.as_str()));
    if matches("FORCE_HYPERLINK", &["1", "true", "always", "enabled"]) {
        return true;
    }
    if let Some(vte) = env("VTE_VERSION") {
        return scan_version(&vte)[0] > 5000;
    }
    if let Some(term) = env("TERM_PROGRAM") {
        let [major, minor, _] = scan_version(&env("TERM_PROGRAM_VERSION").unwrap_or_default());
        match term.as_str() {
            "iTerm.app" => return if major == 3 { minor >= 1 } else { major > 3 },
            "WezTerm" => return major >= 20200620,
            "vscode" => return major > 1 || major == 1 && minor >= 72,
            "ghostty" => return major >= 1,
            _ => {}
        }
    }
    matches(
        "TERM",
        &[
            "xterm-kitty",
            "alacritty",
            "alacritty-direct",
            "xterm-ghostty",
        ],
    ) || matches("COLORTERM", &["xfce4-terminal"])
        || matches("TERMINAL_EMULATOR", &["JetBrains-JediTerm"])
        || ["DOMTERM", "WT_SESSION", "KONSOLE_VERSION"]
            .iter()
            .any(|v| env(v).is_some())
}

/// fmt.Sscanf(s, "%d.%d.%d", ...): the components scanned before the
/// first failure, zero for the rest.
fn scan_version(s: &str) -> [i64; 3] {
    let mut out = [0i64; 3];
    let mut chars = s.chars().peekable();
    for i in 0..3 {
        if i > 0 && chars.next() != Some('.') {
            break;
        }
        // Sscanf skips spaces before a number but stops at a newline.
        while let Some(&c) = chars.peek() {
            if c == '\n' {
                return out;
            }
            if !c.is_whitespace() {
                break;
            }
            chars.next();
            if c == '\r' && chars.peek() == Some(&'\n') {
                return out;
            }
        }
        let mut token = String::new();
        if let Some(&c @ ('+' | '-')) = chars.peek() {
            token.push(c);
            chars.next();
        }
        while let Some(&c @ '0'..='9') = chars.peek() {
            token.push(c);
            chars.next();
        }
        match token.parse::<i64>() {
            Ok(v) => out[i] = v,
            Err(_) => break,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }

    const PLAIN: &str = "Github/eat-pray-ai/yutu (https://github.com/eat-pray-ai/yutu)\x1b[0m";
    const OSC8: &str =
        "\x1b]8;;https://github.com/eat-pray-ai/yutu\x07Github/eat-pray-ai/yutu\x1b]8;;\x07\x1b[0m";

    fn star(vars: &[(&str, &str)]) -> String {
        link(REPO, REPO_URL, &with(vars))
    }

    #[test]
    fn links_follow_termlink_heuristics() {
        assert_eq!(star(&[]), PLAIN);
        assert_eq!(star(&[("FORCE_HYPERLINK", "1")]), OSC8);
        assert_eq!(star(&[("FORCE_HYPERLINK", "yes")]), PLAIN);
        assert_eq!(star(&[("VTE_VERSION", "5000")]), PLAIN);
        assert_eq!(star(&[("VTE_VERSION", "6003")]), OSC8);
        assert_eq!(
            star(&[("VTE_VERSION", "0.60.3"), ("TERM", "xterm-kitty")]),
            PLAIN
        );
        assert_eq!(
            star(&[
                ("TERM_PROGRAM", "iTerm.app"),
                ("TERM_PROGRAM_VERSION", "3.5.2")
            ]),
            OSC8
        );
        assert_eq!(
            star(&[
                ("TERM_PROGRAM", "iTerm.app"),
                ("TERM_PROGRAM_VERSION", "3.0.9")
            ]),
            PLAIN
        );
        assert_eq!(
            star(&[
                ("TERM_PROGRAM", "vscode"),
                ("TERM_PROGRAM_VERSION", "1.72.0")
            ]),
            OSC8
        );
        assert_eq!(
            star(&[("TERM_PROGRAM", "Apple_Terminal"), ("TERM", "alacritty")]),
            OSC8
        );
        assert_eq!(star(&[("COLORTERM", "xfce4-terminal")]), OSC8);
        assert_eq!(star(&[("WT_SESSION", "")]), OSC8);
    }

    #[test]
    fn scans_versions_like_sscanf() {
        assert_eq!(scan_version("3.5.2"), [3, 5, 2]);
        assert_eq!(scan_version("5000"), [5000, 0, 0]);
        assert_eq!(scan_version(" 1.72"), [1, 72, 0]);
        assert_eq!(scan_version("1.x.3"), [1, 0, 0]);
        assert_eq!(scan_version("+7.-2.3"), [7, -2, 3]);
        assert_eq!(scan_version("v1.2"), [0, 0, 0]);
        assert_eq!(scan_version("\n5"), [0, 0, 0]);
        assert_eq!(scan_version("99999999999999999999"), [0, 0, 0]);
    }
}
