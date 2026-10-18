#![forbid(unsafe_code)]

use std::process::Command;

/// The golden banner was captured on darwin/arm64.
const PLATFORM: &str = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
    "darwin/arm64"
} else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
    "darwin/amd64"
} else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
    "linux/arm64"
} else {
    "linux/amd64"
};

fn golden_stderr(case: &str) -> String {
    let path = format!(
        "{}/../../parity/golden/cases/{case}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let golden: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    golden["stderr"]
        .as_str()
        .unwrap()
        .replace("darwin/arm64", PLATFORM)
}

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

#[test]
fn prints_the_go_banner_to_stderr() {
    assert_eq!(
        version(&[]),
        (String::new(), golden_stderr("core.version"), Some(0))
    );
}

#[test]
fn links_the_repository_when_hyperlinks_are_forced() {
    assert_eq!(
        version(&[("FORCE_HYPERLINK", "1")]),
        (
            String::new(),
            golden_stderr("core.version.hyperlink"),
            Some(0)
        )
    );
}
