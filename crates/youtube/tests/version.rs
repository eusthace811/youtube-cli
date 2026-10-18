#![forbid(unsafe_code)]

use std::process::Command;

const PLATFORM: &str = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
    "darwin/arm64"
} else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
    "darwin/amd64"
} else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
    "linux/arm64"
} else {
    "linux/amd64"
};

fn version(env: &[(&str, &str)]) -> (String, String, Option<i32>) {
    let out = Command::new(env!("CARGO_BIN_EXE_youtube"))
        .arg("version")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .envs(env.iter().copied())
        .output()
        .unwrap();
    (
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
        out.status.code(),
    )
}

/// Divergence E19: a plain banner on stderr, as Go printed its own, with
/// no mascot and no link to the Go project.
#[test]
fn prints_the_banner_to_stderr() {
    let (stdout, stderr, code) = version(&[]);
    assert_eq!((stdout.as_str(), code), ("", Some(0)));
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(lines.len(), 3, "{stderr}");
    assert!(lines[0].starts_with("youtube-cli v"), "{stderr}");
    assert!(lines[0].ends_with(PLATFORM), "{stderr}");
    assert_eq!(
        lines[1],
        "CLI, MCP server, and AI agent to automate YouTube workflows"
    );
    assert!(lines[2].starts_with("build: "), "{stderr}");
}

#[test]
fn terminal_hints_do_not_change_it() {
    assert_eq!(version(&[("FORCE_HYPERLINK", "1")]), version(&[]));
}
