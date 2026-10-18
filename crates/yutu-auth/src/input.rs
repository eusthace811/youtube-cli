//! Resolves `--credential` and `--cacheToken` the way Go's `WithCredential`
//! and `WithCacheToken` do: a file under the root, else standard base64,
//! else a JSON string.

use crate::root::RootDir;
use crate::{gobase64, gojson, gopath, log};
use std::fmt;
use std::io;

pub const DEFAULT_CREDENTIAL: &str = "client_secret.json";
pub const DEFAULT_CACHE_TOKEN: &str = "youtube.token.json";

const AUTH_HINT: &str = "Please configure client secret as described in https://github.com/eat-pray-ai/yutu#prerequisites";

/// Credential or token bytes. `Debug` and `Display` never show them, so a
/// secret cannot reach a log line by formatting.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secret(Vec<u8>);

impl Secret {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Secret {
        Secret(bytes.into())
    }

    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([redacted])")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

/// A resolved `--cacheToken`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CacheToken {
    /// The token JSON; empty when there is none yet.
    pub json: Secret,
    /// Where refreshed and new tokens are saved, relative to the root.
    pub file: Option<String>,
}

/// The flag value when non-empty, else the env var when set (even empty),
/// else the default, as Go's options pick it.
fn pick(flag: &str, env: &str, default: &str) -> String {
    if !flag.is_empty() {
        return flag.to_string();
    }
    match std::env::var_os(env) {
        Some(v) => v.to_string_lossy().into_owned(),
        None => default.to_string(),
    }
}

/// The argument as a root-relative path, as Go computes it with
/// `filepath.Abs` then `filepath.Rel`. Empty when Go's `Rel` fails.
fn under_root(root: &RootDir, arg: &str) -> (String, String) {
    let abs = gopath::abs(arg).unwrap_or_default();
    let rel = gopath::rel(root.path(), &abs)
        .unwrap_or_default()
        .replace('\\', "/");
    (abs, rel)
}

/// `WithCredential`: the client secret JSON, or Go's init error text.
pub fn resolve_credential(root: &RootDir, flag: &str) -> Result<Secret, String> {
    let cred = pick(flag, "YUTU_CREDENTIAL", DEFAULT_CREDENTIAL);
    let (abs, rel) = under_root(root, &cred);
    if root.stat(&rel).is_ok() {
        return match root.read_unblocked(&rel) {
            Ok(bytes) => Ok(Secret(bytes)),
            Err(e) => {
                log::error(
                    "failed to read client secret",
                    &[("hint", &AUTH_HINT), ("path", &abs), ("error", &e)],
                );
                Err(format!("failed to read client secret: {e}"))
            }
        };
    }
    decode_inline(&cred).map_err(|e| {
        log::error(
            "failed to parse client secret",
            &[("hint", &AUTH_HINT), ("error", &e)],
        );
        format!("failed to parse client secret: {e}")
    })
}

/// `WithCacheToken`. Unlike Go, a value that parses as nothing is logged
/// without the value itself (crosscut 11).
pub fn resolve_cache_token(root: &RootDir, flag: &str) -> CacheToken {
    let token = pick(flag, "YUTU_CACHE_TOKEN", DEFAULT_CACHE_TOKEN);
    let (abs, rel) = under_root(root, &token);
    match root.stat(&rel) {
        Ok(_) => {
            let json = root.read_unblocked(&rel).unwrap_or_else(|e| {
                log::warn("failed to read token", &[("path", &abs), ("error", &e)]);
                Vec::new()
            });
            return CacheToken {
                json: Secret(json),
                file: Some(rel),
            };
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound && token.ends_with(".json") => {
            return CacheToken {
                json: Secret::default(),
                file: Some(rel),
            };
        }
        Err(_) => {}
    }
    match decode_inline(&token) {
        Ok(json) => CacheToken { json, file: None },
        Err(e) => {
            log::warn("failed to parse token", &[("error", &e)]);
            CacheToken::default()
        }
    }
}

/// An argument that is not a file: standard base64 (URL-safe is rejected),
/// else JSON. The error is the base64 one, as Go reports it.
pub fn decode_inline(arg: &str) -> Result<Secret, gobase64::CorruptInput> {
    match gobase64::std_decode(arg.as_bytes()) {
        Ok(decoded) => Ok(Secret(decoded)),
        Err(_) if gojson::is_json(arg.as_bytes()) => Ok(Secret(arg.as_bytes().to_vec())),
        Err(e) => Err(e),
    }
}
