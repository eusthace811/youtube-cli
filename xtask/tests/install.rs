#![cfg(unix)]
#![forbid(unsafe_code)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const FAKE_YUTU: &str = "#!/bin/sh\necho \"yutu v9.9.9-test $1\"\n";

fn asset() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        os => os,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        arch => arch,
    };
    format!("yutu-{os}-{arch}")
}

fn sha256(path: &Path) -> String {
    let out = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap()[..64].to_string()
}

/// A release directory holding the asset and a SHA256SUMS listing `listed`.
fn release(name: &str, listed: &[u8], shipped: &[u8]) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("install")
        .join(name);
    let _ = fs::remove_dir_all(&dir);
    let rel = dir.join("release");
    fs::create_dir_all(&rel).unwrap();
    let bin = rel.join(asset());
    fs::write(&bin, listed).unwrap();
    let sums = format!(
        "{}  {}\n{}  yutu-other-arch\n",
        sha256(&bin),
        asset(),
        "0".repeat(64)
    );
    fs::write(rel.join("SHA256SUMS"), sums).unwrap();
    fs::write(&bin, shipped).unwrap();
    dir
}

/// Runs install.sh with a fake `npx` first on PATH that records its arguments.
fn install(dir: &Path, args: &[&str]) -> Output {
    let fake = dir.join("fakebin");
    fs::create_dir_all(&fake).unwrap();
    let npx = fake.join("npx");
    fs::write(
        &npx,
        format!(
            "#!/bin/sh\necho \"$*\" > '{}'\n",
            dir.join("npx.log").display()
        ),
    )
    .unwrap();
    fs::set_permissions(&npx, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", fake.display(), std::env::var("PATH").unwrap());
    Command::new("bash")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../install.sh"))
        .args(args)
        .env("PATH", path)
        .env(
            "YUTU_RELEASE_URL",
            format!("file://{}", dir.join("release").display()),
        )
        .env("XDG_BIN_HOME", dir.join("bin"))
        .env("HOME", dir.join("home"))
        .env("YUTU_SKIP_ATTESTATION", "1")
        .env("YUTU_REPO", "owner/yutu-rs")
        .output()
        .unwrap()
}

#[test]
fn a_tampered_binary_fails_with_checksum_mismatch() {
    let dir = release("tampered", FAKE_YUTU.as_bytes(), b"#!/bin/sh\necho pwned\n");
    let out = install(&dir, &[]);
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.starts_with(&format!("checksum mismatch: {} has sha256 ", asset())),
        "{stderr}"
    );
    assert!(!dir.join("bin/yutu").exists());
}

#[test]
fn a_clean_release_installs_and_prints_the_version() {
    let dir = release("clean", FAKE_YUTU.as_bytes(), FAKE_YUTU.as_bytes());
    let out = install(&dir, &[]);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bin = dir.join("bin/yutu");
    let expected = format!(
        "checksum verified: {asset} sha256 {sum}\n\
         attestation skipped: YUTU_SKIP_ATTESTATION=1\n\
         yutu v9.9.9-test version\n\
         \n\
         yutu🐰 installed:\n  {bin}\n\nAdd {dir} to PATH if needed.\n",
        asset = asset(),
        sum = sha256(&bin),
        bin = bin.display(),
        dir = dir.join("bin").display(),
    );
    assert_eq!(stdout, expected);
    assert_eq!(fs::read(&bin).unwrap(), FAKE_YUTU.as_bytes());
    assert_eq!(
        fs::metadata(&bin).unwrap().permissions().mode() & 0o111,
        0o111
    );
    assert!(
        !dir.join("npx.log").exists(),
        "no skill without --with-skill"
    );
}

#[test]
fn with_skill_installs_the_skill_through_npx() {
    let dir = release("skill", FAKE_YUTU.as_bytes(), FAKE_YUTU.as_bytes());
    let out = install(&dir, &["--with-skill"]);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    assert!(
        stdout.contains("Installing skill...\nSkill installed.\n"),
        "{stdout}"
    );
    assert_eq!(
        fs::read_to_string(dir.join("npx.log")).unwrap(),
        "--yes skills add owner/yutu-rs --skill youtube -y\n"
    );
}

#[test]
fn without_a_repository_it_refuses_to_guess() {
    let out = Command::new("bash")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../install.sh"))
        .env_remove("YUTU_REPO")
        .env_remove("YUTU_RELEASE_URL")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "Set YUTU_REPO to the GitHub repository of the yutu-rs releases (owner/name).\n\
         The Go project's releases (eat-pray-ai/yutu) are a different program.\n"
    );
}
