#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::Command;

fn fixture(name: &str, contents: &[u8]) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("egress");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    path
}

fn egress(path: &PathBuf) -> (Option<i32>, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("egress")
        .arg(path)
        .output()
        .unwrap();
    (
        out.status.code(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

#[test]
fn fails_on_a_telemetry_host() {
    let path = fixture(
        "telemetry.bin",
        b"\x7fELF\0\0\x01report to https://telemetry.example.com/v1/events\0\x02",
    );
    assert_eq!(
        egress(&path),
        (
            Some(1),
            "egress: unexpected host telemetry.example.com: https://telemetry.example.com/v1/events\n".to_string(),
            "egress: 1 unexpected hosts\n".to_string()
        )
    );
}

#[test]
fn passes_allowed_and_display_only_urls() {
    let path = fixture(
        "allowed.bin",
        b"\0https://oauth2.googleapis.com/token\0http://localhost:8216\0Star https://github.com/eat-pray-ai/yutuv0.11.0\0",
    );
    assert_eq!(
        egress(&path),
        (
            Some(0),
            "egress: display-only https://github.com/eat-pray-ai/yutu (the `youtube version` banner link)\negress: 0 unexpected hosts\n"
                .to_string(),
            String::new()
        )
    );
}
