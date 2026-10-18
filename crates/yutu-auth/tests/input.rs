mod common;

use common::{TempDir, without_time};
use std::process::Command;
use yutu_auth::input::{CacheToken, resolve_cache_token, resolve_credential};
use yutu_auth::{RootDir, Secret, gobase64, log};

const CRED: &str = r#"{"installed":{"client_id":"id","redirect_uris":["http://localhost"]}}"#;
const TOKEN: &str = r#"{"access_token":"ya29.SECRET-ACCESS","refresh_token":"1//SECRET-REFRESH","expiry":"2026-01-02T03:04:05Z","expires_in":0}"#;

fn root(dir: &TempDir) -> RootDir {
    RootDir::open(dir.str()).unwrap()
}

#[test]
fn credential_from_a_file_under_the_root() {
    let dir = TempDir::new();
    std::fs::write(dir.join("client_secret.json"), CRED).unwrap();
    let path = dir.join("client_secret.json");
    let got = resolve_credential(&root(&dir), path.to_str().unwrap()).unwrap();
    assert_eq!(got.expose(), CRED.as_bytes());
}

#[test]
fn credential_from_standard_base64() {
    let dir = TempDir::new();
    let b64 = gobase64::std_encode(CRED.as_bytes());
    let got = resolve_credential(&root(&dir), &b64).unwrap();
    assert_eq!(got.expose(), CRED.as_bytes());
}

#[test]
fn credential_from_a_json_string() {
    let dir = TempDir::new();
    let got = resolve_credential(&root(&dir), CRED).unwrap();
    assert_eq!(got.expose(), CRED.as_bytes());
}

#[test]
fn url_safe_base64_is_rejected() {
    let dir = TempDir::new();
    let (result, logs) = log::capture(|| resolve_credential(&root(&dir), "eyJ-_w=="));
    assert_eq!(
        result.unwrap_err(),
        "failed to parse client secret: illegal base64 data at input byte 3"
    );
    assert_eq!(
        without_time(&logs),
        "level=ERROR msg=\"failed to parse client secret\" hint=\"Please configure client secret as described in https://github.com/eat-pray-ai/yutu#prerequisites\" error=\"illegal base64 data at input byte 3\""
    );
}

#[test]
fn a_file_outside_the_root_is_not_read() {
    let dir = TempDir::new();
    let outside = TempDir::new();
    std::fs::write(outside.join("client_secret.json"), CRED).unwrap();
    let path = outside.join("client_secret.json");
    let path = path.to_str().unwrap();
    let (result, _) = log::capture(|| resolve_credential(&root(&dir), path));
    // Go then tries the path as base64, which fails at its first character
    // outside the standard alphabet.
    let bad = path
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '+' || c == '/'))
        .unwrap();
    assert_eq!(
        result.unwrap_err(),
        format!("failed to parse client secret: illegal base64 data at input byte {bad}")
    );
}

#[test]
fn token_forms() {
    let dir = TempDir::new();
    let root = root(&dir);
    std::fs::write(dir.join("tok.json"), TOKEN).unwrap();
    let file = dir.join("tok.json");
    assert_eq!(
        resolve_cache_token(&root, file.to_str().unwrap()),
        CacheToken {
            json: Secret::new(TOKEN),
            file: Some("tok.json".to_string()),
        }
    );
    assert_eq!(
        resolve_cache_token(&root, &gobase64::std_encode(TOKEN.as_bytes())),
        CacheToken {
            json: Secret::new(TOKEN),
            file: None,
        }
    );
    assert_eq!(
        resolve_cache_token(&root, TOKEN),
        CacheToken {
            json: Secret::new(TOKEN),
            file: None,
        }
    );
    let missing = dir.join("sub/new.token.json");
    assert_eq!(
        resolve_cache_token(&root, missing.to_str().unwrap()),
        CacheToken {
            json: Secret::default(),
            file: Some("sub/new.token.json".to_string()),
        }
    );
}

#[test]
fn garbage_token_is_logged_without_its_text() {
    let dir = TempDir::new();
    let garbage = r#"{"access_token":"ya29.SECRET-ACCESS""#;
    let (result, logs) = log::capture(|| resolve_cache_token(&root(&dir), garbage));
    assert_eq!(result, CacheToken::default());
    assert_eq!(
        without_time(&logs),
        "level=WARN msg=\"failed to parse token\" error=\"illegal base64 data at input byte 0\""
    );
    assert!(!logs.contains("SECRET"));
}

#[test]
fn secrets_never_format() {
    let secret = Secret::new(TOKEN);
    assert_eq!(
        format!("{secret} {secret:?}"),
        "[redacted] Secret([redacted])"
    );
    let dir = TempDir::new();
    let token = resolve_cache_token(&root(&dir), TOKEN);
    assert!(!format!("{token:?}").contains("SECRET"));
}

/// Runs `child_case` in a child test process whose working directory is the
/// root and whose environment holds `envs`, since both are process-wide.
fn in_child(dir: &TempDir, flag: &str, envs: &[(&str, &str)]) -> String {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", "child_case", "--nocapture", "--test-threads=1"])
        .current_dir(dir.path())
        .env("YUTU_TEST_CHILD_FLAG", flag)
        .env_remove("YUTU_CREDENTIAL")
        .env_remove("YUTU_CACHE_TOKEN")
        .env_remove("YUTU_LOG_LEVEL")
        .env_remove("PWD");
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    stdout
        .lines()
        .filter_map(|l| l.find("CHILD ").map(|i| &l[i + 6..]))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn child_case() {
    let Ok(flag) = std::env::var("YUTU_TEST_CHILD_FLAG") else {
        return;
    };
    let root = RootDir::open(&yutu_auth::gopath::getwd().unwrap()).unwrap();
    let ((cred, token), logs) = log::capture(|| {
        (
            resolve_credential(&root, &flag),
            resolve_cache_token(&root, &flag),
        )
    });
    let cred = cred.map(|s| String::from_utf8(s.expose().to_vec()).unwrap());
    let json = String::from_utf8(token.json.expose().to_vec()).unwrap();
    println!("CHILD cred={cred:?}");
    println!("CHILD token={json:?} file={:?}", token.file);
    for line in without_time(&logs).lines() {
        println!("CHILD {line}");
    }
}

#[test]
fn defaults_resolve_against_the_working_directory() {
    let dir = TempDir::new();
    assert_eq!(
        in_child(&dir, "", &[]),
        "cred=Err(\"failed to parse client secret: illegal base64 data at input byte 6\")\n\
         token=\"\" file=Some(\"youtube.token.json\")\n\
         level=ERROR msg=\"failed to parse client secret\" hint=\"Please configure client secret as described in https://github.com/eat-pray-ai/yutu#prerequisites\" error=\"illegal base64 data at input byte 6\""
    );
    std::fs::write(dir.join("client_secret.json"), CRED).unwrap();
    std::fs::write(dir.join("youtube.token.json"), TOKEN).unwrap();
    assert_eq!(
        in_child(&dir, "", &[]),
        format!("cred=Ok({CRED:?})\ntoken={TOKEN:?} file=Some(\"youtube.token.json\")")
    );
}

#[test]
fn env_applies_only_when_the_flag_is_empty() {
    let dir = TempDir::new();
    let env_token = gobase64::std_encode(TOKEN.as_bytes());
    let envs = [
        ("YUTU_CREDENTIAL", CRED),
        ("YUTU_CACHE_TOKEN", env_token.as_str()),
    ];
    assert_eq!(
        in_child(&dir, "", &envs),
        format!("cred=Ok({CRED:?})\ntoken={TOKEN:?} file=None")
    );
    let flag = r#"{"flag":true}"#;
    assert_eq!(
        in_child(&dir, flag, &envs),
        format!("cred=Ok({flag:?})\ntoken={flag:?} file=None")
    );
}

#[test]
fn garbage_env_token_stays_out_of_debug_logs() {
    let dir = TempDir::new();
    let garbage = "SECRET-not-base64-not-json";
    let out = in_child(
        &dir,
        "",
        &[
            ("YUTU_CACHE_TOKEN", garbage),
            ("YUTU_CREDENTIAL", CRED),
            ("YUTU_LOG_LEVEL", "DEBUG"),
        ],
    );
    assert_eq!(
        out,
        format!(
            "cred=Ok({CRED:?})\ntoken=\"\" file=None\nlevel=WARN msg=\"failed to parse token\" error=\"illegal base64 data at input byte 6\""
        )
    );
    assert!(!out.contains("SECRET"));
}
