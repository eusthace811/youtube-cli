//! Bearer-token checks for `youtube mcp --mode http`: go-sdk's
//! `RequireBearerToken` over youtube's Google tokeninfo verifier.
//!
//! Unlike Go, the token travels in a POST body instead of the URL, an
//! optional client allowlist checks `aud`/`azp`, and verified tokens are
//! cached until they expire (divergence E4).

use ring::digest::{SHA256, digest};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};
use youtube_api::Service;
use youtube_api::client::{BoxFuture, TokenSource};
use youtube_auth::Secret;
use youtube_core::ops::ServiceProvider;

pub const TOKENINFO_URL: &str = "https://oauth2.googleapis.com/tokeninfo";

/// `auth.Scopes`: a token must hold all three.
pub const SCOPES: [&str; 3] = [
    "https://www.googleapis.com/auth/youtube",
    "https://www.googleapis.com/auth/youtube.force-ssl",
    "https://www.googleapis.com/auth/youtube.channel-memberships.creator",
];

const CACHE_CAPACITY: usize = 1024;
const MAX_TOKENINFO_BODY: usize = 64 << 10;

/// The tokeninfo endpoint: Google's, unless a `parity-endpoint` build points
/// `YUTU_PARITY_TOKENINFO` at this machine.
pub fn tokeninfo_url() -> String {
    #[cfg(feature = "parity-endpoint")]
    if let Ok(u) = std::env::var("YUTU_PARITY_TOKENINFO")
        && is_loopback_url(&u)
    {
        return u;
    }
    TOKENINFO_URL.to_string()
}

/// Mirrors youtube-api's rule for `YUTU_PARITY_ENDPOINT`: a stray variable can
/// never send bearer tokens off this machine.
#[cfg(feature = "parity-endpoint")]
fn is_loopback_url(url: &str) -> bool {
    let rest = url.strip_prefix("http://").unwrap_or("");
    let auth = rest.split(['/', '?', '#']).next().unwrap_or("");
    if auth.contains(['@', '\\', '%']) {
        return false;
    }
    let host = match auth.rsplit_once(':') {
        Some((h, port)) if !h.ends_with(':') && port.bytes().all(|b| b.is_ascii_digit()) => h,
        _ => auth,
    };
    matches!(host, "127.0.0.1" | "localhost" | "[::1]")
}

/// `YOUTUBE_CLI_MCP_ALLOWED_CLIENT_IDS`, comma separated; `None` accepts any client.
pub fn allowed_clients_from_env() -> Option<Vec<String>> {
    let v = std::env::var("YOUTUBE_CLI_MCP_ALLOWED_CLIENT_IDS").ok()?;
    let ids: Vec<String> = v
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    (!ids.is_empty()).then_some(ids)
}

/// Why a request was refused: the HTTP status and go-sdk's `http.Error` text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denial {
    pub status: u16,
    pub message: String,
}

impl Denial {
    fn invalid(detail: impl std::fmt::Display) -> Denial {
        Denial {
            status: 401,
            message: format!("invalid token: {detail}"),
        }
    }

    fn unauthorized(message: &str) -> Denial {
        Denial {
            status: 401,
            message: message.to_string(),
        }
    }
}

/// A verified caller. Tool calls reach YouTube with this token and nothing
/// else, so the local token file is never consulted in HTTP mode.
pub struct Caller {
    token: Secret,
    expires: Instant,
    api_base: String,
    service: OnceLock<Service>,
}

impl ServiceProvider for Caller {
    fn service(&self) -> BoxFuture<'_, Result<Service, String>> {
        let svc = self
            .service
            .get_or_init(|| {
                let tokens = Arc::new(StaticToken(self.token.clone()));
                Service::with_base(self.api_base.clone(), Some(tokens))
            })
            .clone();
        Box::pin(async move { Ok(svc) })
    }
}

struct StaticToken(Secret);

impl TokenSource for StaticToken {
    fn token(&self) -> BoxFuture<'_, Result<String, String>> {
        let t = String::from_utf8_lossy(self.0.expose()).into_owned();
        Box::pin(async move { Ok(t) })
    }
}

pub struct Verifier {
    http: reqwest::Client,
    url: String,
    allowed: Option<Vec<String>>,
    api_base: String,
    cache: Mutex<Lru>,
}

#[derive(Deserialize)]
struct TokenInfo {
    #[serde(default)]
    expires_in: Option<String>,
    #[serde(default)]
    scope: String,
    #[serde(default)]
    aud: String,
    #[serde(default)]
    azp: String,
    #[serde(default)]
    error: String,
}

impl Verifier {
    /// `api_base` is the YouTube API base URL callers' services use.
    pub fn new(url: String, allowed: Option<Vec<String>>, api_base: String) -> Verifier {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("TLS backend available");
        Verifier {
            http,
            url,
            allowed,
            api_base,
            cache: Mutex::new(Lru::default()),
        }
    }

    /// go-sdk's `verify`: the `Authorization` header in, the caller or the
    /// refusal out.
    pub async fn verify(&self, authorization: Option<&[u8]>) -> Result<Arc<Caller>, Denial> {
        let header = authorization
            .map(String::from_utf8_lossy)
            .unwrap_or_default();
        let fields: Vec<&str> = header.split_ascii_whitespace().collect();
        let [scheme, token] = fields[..] else {
            return Err(Denial::unauthorized("no bearer token"));
        };
        if !scheme.eq_ignore_ascii_case("bearer") {
            return Err(Denial::unauthorized("no bearer token"));
        }
        let key = digest(&SHA256, token.as_bytes());
        let key: [u8; 32] = key.as_ref().try_into().expect("SHA-256 is 32 bytes");
        if let Some(c) = self.lock().get(&key) {
            return Ok(c);
        }
        let info = self.tokeninfo(token).await?;
        if let Some(allowed) = &self.allowed
            && !allowed.iter().any(|id| *id == info.aud || *id == info.azp)
        {
            return Err(Denial::invalid("audience not allowed"));
        }
        let granted: Vec<&str> = info.scope.split_ascii_whitespace().collect();
        if SCOPES.iter().any(|s| !granted.contains(s)) {
            return Err(Denial {
                status: 403,
                message: "insufficient scope".into(),
            });
        }
        // Go stamps `now + expires_in` and rejects it as already past when
        // the field is absent or zero.
        let secs = match info.expires_in.as_deref() {
            None => 0,
            Some(s) => s
                .parse::<i64>()
                .map_err(|_| Denial::invalid("tokeninfo expires_in is not a number"))?,
        };
        if secs <= 0 {
            return Err(Denial::unauthorized("token expired"));
        }
        let caller = Arc::new(Caller {
            token: Secret::new(token),
            // A day at most: tokeninfo never grants more than an hour, and
            // a hostile value must not overflow the clock.
            expires: Instant::now() + Duration::from_secs(secs.unsigned_abs().min(86_400)),
            api_base: self.api_base.clone(),
            service: OnceLock::new(),
        });
        self.lock().insert(key, caller.clone());
        Ok(caller)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Lru> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
    }

    async fn tokeninfo(&self, token: &str) -> Result<TokenInfo, Denial> {
        let body = format!("access_token={}", form_encode(token));
        let mut resp = self
            .http
            .post(&self.url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .map_err(|e| Denial::invalid(e.without_url()))?;
        let status = resp.status().as_u16();
        if status != 200 {
            return Err(Denial::invalid(format_args!("tokeninfo returned {status}")));
        }
        let mut buf = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(Denial::invalid)? {
            if buf.len() + chunk.len() > MAX_TOKENINFO_BODY {
                return Err(Denial::invalid("tokeninfo response too large"));
            }
            buf.extend_from_slice(&chunk);
        }
        let info: TokenInfo = serde_json::from_slice(&buf).map_err(Denial::invalid)?;
        if !info.error.is_empty() {
            return Err(Denial::invalid(&info.error));
        }
        Ok(info)
    }
}

/// `application/x-www-form-urlencoded` for one value.
fn form_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'*' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Verified callers by token digest, least recently used evicted first.
#[derive(Default)]
struct Lru {
    entries: HashMap<[u8; 32], (Arc<Caller>, u64)>,
    tick: u64,
}

impl Lru {
    fn get(&mut self, key: &[u8; 32]) -> Option<Arc<Caller>> {
        self.tick += 1;
        let now = Instant::now();
        let (caller, used) = self.entries.get_mut(key)?;
        if caller.expires <= now {
            self.entries.remove(key);
            return None;
        }
        *used = self.tick;
        Some(caller.clone())
    }

    fn insert(&mut self, key: [u8; 32], caller: Arc<Caller>) {
        self.tick += 1;
        if self.entries.len() >= CACHE_CAPACITY && !self.entries.contains_key(&key) {
            let now = Instant::now();
            self.entries.retain(|_, (c, _)| c.expires > now);
            if self.entries.len() >= CACHE_CAPACITY
                && let Some(oldest) = self
                    .entries
                    .iter()
                    .min_by_key(|(_, (_, used))| *used)
                    .map(|(k, _)| *k)
            {
                self.entries.remove(&oldest);
            }
        }
        self.entries.insert(key, (caller, self.tick));
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caller(secs: u64) -> Arc<Caller> {
        Arc::new(Caller {
            token: Secret::new("t"),
            expires: Instant::now() + Duration::from_secs(secs),
            api_base: String::new(),
            service: OnceLock::new(),
        })
    }

    fn key(i: usize) -> [u8; 32] {
        let mut k = [0u8; 32];
        k[..8].copy_from_slice(&(i as u64).to_le_bytes());
        k
    }

    #[test]
    fn evicts_the_least_recently_used_at_capacity() {
        let mut lru = Lru::default();
        for i in 0..CACHE_CAPACITY {
            lru.insert(key(i), caller(60));
        }
        assert!(lru.get(&key(0)).is_some());
        lru.insert(key(CACHE_CAPACITY), caller(60));
        assert_eq!(lru.len(), CACHE_CAPACITY);
        assert!(lru.get(&key(0)).is_some(), "recently used entry kept");
        assert!(lru.get(&key(1)).is_none(), "oldest entry evicted");
    }

    #[test]
    fn expired_entries_miss() {
        let mut lru = Lru::default();
        lru.insert(key(1), caller(0));
        assert!(lru.get(&key(1)).is_none());
        assert_eq!(lru.len(), 0);
    }

    #[test]
    fn form_encoding_escapes_reserved_bytes() {
        assert_eq!(form_encode("ya29.a-b_c"), "ya29.a-b_c");
        assert_eq!(form_encode("a+b/c=&d e"), "a%2Bb%2Fc%3D%26d+e");
    }
}

#[cfg(all(test, feature = "parity-endpoint"))]
mod loopback_tests {
    use super::is_loopback_url;

    #[test]
    fn only_a_plain_loopback_authority_counts() {
        assert!(is_loopback_url("http://127.0.0.1:8080/tokeninfo"));
        assert!(is_loopback_url("http://[::1]:8080/"));
        assert!(is_loopback_url("http://localhost/"));
        assert!(!is_loopback_url("http://localhost:1@evil.example/"));
        assert!(!is_loopback_url("http://localhost@evil.example/"));
        assert!(!is_loopback_url("http://localhost.evil.example/"));
        assert!(!is_loopback_url("https://127.0.0.1/"));
    }
}
