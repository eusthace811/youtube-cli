//! The installed-app OAuth flow of Go's pkg/auth/auth.go and the token
//! endpoint calls of golang.org/x/oauth2: PKCE S256 consent in the browser,
//! a loopback callback on the redirect host, and a pasted-code fallback.

use crate::token::{GoTime, Token};
use crate::{gobase64, log, root};
use serde_json::Value;
use std::fmt;
use std::io::{self, Read, Write};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, mpsc};

pub const SCOPES: [&str; 3] = [
    "https://www.googleapis.com/auth/youtube",
    "https://www.googleapis.com/auth/youtube.force-ssl",
    "https://www.googleapis.com/auth/youtube.channel-memberships.creator",
];

/// `common.DefaultRedirectURL`; it replaces the client secret's own redirect.
pub const DEFAULT_REDIRECT_URL: &str = "http://localhost:8216";
pub const CALLBACK_TIMEOUT: Duration = Duration::from_secs(120);

const STATE_MISMATCH: &str = "state doesn't match, possible CSRF attack";
const CODE_ALREADY_RECEIVED: &str = "authorization code already received";
const MAX_TOKEN_BODY: usize = 1 << 20;
const MAX_REQUEST_HEAD: usize = 16 << 10;
const REQUEST_HEAD_TIMEOUT: Duration = Duration::from_secs(10);

fn browser_opened_hint(url: &str) -> String {
    format!(
        "Your browser has been opened to an authorization URL. yutu will resume once authorization has been provided.\n{url}\n"
    )
}

fn open_browser_hint(url: &str) -> String {
    format!(
        "It seems that your browser is not open. Go to the following link in your browser:\n{url}\n"
    )
}

fn manual_input_hint(redirect_url: &str) -> String {
    format!(
        "
After completing the authorization flow, enter the authorization code on command line.

If you end up in an error page after completing the authorization flow,
and the url in the address bar is in the form of
'{redirect_url}/?state=DONOT-COPY&code=COPY-THIS&scope=DONOT-COPY'
ONLY 'COPY-THIS' part is the code you need to enter on command line.
"
    )
}

static MCP_STDIO: AtomicBool = AtomicBool::new(false);

/// Marks stdout as a stdio MCP stream. From then on a login prints its hints
/// to stderr and never reads stdin, so the JSON-RPC channel stays intact
/// (divergence E2).
pub fn set_mcp_stdio(on: bool) {
    MCP_STDIO.store(on, Ordering::SeqCst);
}

pub fn mcp_stdio() -> bool {
    MCP_STDIO.load(Ordering::SeqCst)
}

/// An OAuth client from a Google client secret file, as
/// `google.ConfigFromJSON` builds it.
pub struct ClientConfig {
    pub client_id: String,
    client_secret: String,
    pub auth_url: String,
    pub token_url: String,
    pub redirect_url: String,
    /// oauth2's per-config auth style cache: `Some(true)` sends the client
    /// credentials as form parameters, `Some(false)` as basic auth.
    in_params: Mutex<Option<bool>>,
}

impl fmt::Debug for ClientConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientConfig")
            .field("client_id", &self.client_id)
            .field("client_secret", &"[redacted]")
            .field("auth_url", &self.auth_url)
            .field("token_url", &self.token_url)
            .field("redirect_url", &self.redirect_url)
            .finish()
    }
}

impl ClientConfig {
    /// `google.ConfigFromJSON`: the `web` entry, else `installed`. Keys match
    /// case-insensitively, as Go's `encoding/json` (v1) matches them.
    pub fn from_json(data: &[u8]) -> Result<ClientConfig, String> {
        let value: Value = serde_json::from_slice(data).map_err(|e| e.to_string())?;
        let top = match &value {
            Value::Null => return Err("oauth2/google: no credentials found".to_string()),
            Value::Object(m) => m,
            other => {
                return Err(format!(
                    "json: cannot unmarshal {} into Go value",
                    kind(other)
                ));
            }
        };
        let entry = |name: &str| -> Result<Option<&serde_json::Map<String, Value>>, String> {
            match field(top, name) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::Object(m)) => Ok(Some(m)),
                Some(other) => Err(format!(
                    "json: cannot unmarshal {} into Go struct field .{name}",
                    kind(other)
                )),
            }
        };
        let (web, installed) = (entry("web")?, entry("installed")?);
        let cred = web
            .or(installed)
            .ok_or_else(|| "oauth2/google: no credentials found".to_string())?;
        let text = |name: &str| -> Result<String, String> {
            match field(cred, name) {
                None | Some(Value::Null) => Ok(String::new()),
                Some(Value::String(s)) => Ok(s.clone()),
                Some(other) => Err(format!(
                    "json: cannot unmarshal {} into Go struct field cred.{name} of type string",
                    kind(other)
                )),
            }
        };
        let redirect_uris = match field(cred, "redirect_uris") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .map(|v| match v {
                    Value::String(s) => Ok(s.clone()),
                    Value::Null => Ok(String::new()),
                    other => Err(format!("json: cannot unmarshal {} into Go struct field cred.redirect_uris of type string", kind(other))),
                })
                .collect::<Result<_, _>>()?,
            Some(other) => {
                return Err(format!("json: cannot unmarshal {} into Go struct field cred.redirect_uris of type []string", kind(other)));
            }
        };
        let config = ClientConfig {
            client_id: text("client_id")?,
            client_secret: text("client_secret")?,
            auth_url: text("auth_uri")?,
            token_url: text("token_uri")?,
            redirect_url: String::new(),
            in_params: Mutex::new(None),
        };
        let first = redirect_uris.into_iter().next().ok_or_else(|| {
            "oauth2/google: missing redirect URL in the client_credentials.json".to_string()
        })?;
        Ok(ClientConfig {
            redirect_url: first,
            ..config
        })
    }

    /// `Config.AuthCodeURL(state, ApprovalForce, AccessTypeOffline,
    /// S256ChallengeOption(verifier))`.
    pub fn auth_code_url(&self, state: &str, verifier: &str) -> String {
        let mut v = vec![
            ("response_type", "code".to_string()),
            ("client_id", self.client_id.clone()),
            ("scope", SCOPES.join(" ")),
            ("prompt", "consent".to_string()),
            ("access_type", "offline".to_string()),
            ("code_challenge_method", "S256".to_string()),
            ("code_challenge", s256_challenge(verifier)),
        ];
        if !self.redirect_url.is_empty() {
            v.push(("redirect_uri", self.redirect_url.clone()));
        }
        if !state.is_empty() {
            v.push(("state", state.to_string()));
        }
        let sep = if self.auth_url.contains('?') {
            '&'
        } else {
            '?'
        };
        format!("{}{sep}{}", self.auth_url, encode_form(v))
    }
}

fn field<'a>(m: &'a serde_json::Map<String, Value>, name: &str) -> Option<&'a Value> {
    m.get(name).or_else(|| {
        m.iter()
            .rev()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v)
    })
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// `url.QueryEscape`.
pub fn query_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `url.Values.Encode`: keys sorted, each value query-escaped.
fn encode_form(mut v: Vec<(&str, String)>) -> String {
    v.sort_by(|a, b| a.0.cmp(b.0));
    v.iter()
        .map(|(k, val)| format!("{}={}", query_escape(k), query_escape(val)))
        .collect::<Vec<_>>()
        .join("&")
}

/// `url.Values.Set`.
fn set(v: &mut Vec<(&'static str, String)>, key: &'static str, value: String) {
    v.retain(|(k, _)| *k != key);
    v.push((key, value));
}

fn unescape_query(s: &str) -> Result<String, String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let hex = b
                    .get(i + 1..i + 3)
                    .filter(|h| h.iter().all(u8::is_ascii_hexdigit));
                let Some(hex) = hex else {
                    let end = (i + 3).min(b.len());
                    return Err(format!(
                        "invalid URL escape {}",
                        log::quote(&String::from_utf8_lossy(&b[i..end]))
                    ));
                };
                let hex = std::str::from_utf8(hex).expect("ASCII hex digits");
                out.push(u8::from_str_radix(hex, 16).expect("validated hex"));
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// `url.ParseQuery`: every pair that parses, plus the first error.
fn parse_query(query: &str) -> (Vec<(String, String)>, Option<String>) {
    let mut pairs = Vec::new();
    let mut first_err = None;
    for part in query.split('&') {
        if part.is_empty() {
            continue;
        }
        if part.contains(';') {
            first_err.get_or_insert_with(|| "invalid semicolon separator in query".to_string());
            continue;
        }
        let (k, v) = part.split_once('=').unwrap_or((part, ""));
        match (unescape_query(k), unescape_query(v)) {
            (Ok(k), Ok(v)) => pairs.push((k, v)),
            (Err(e), _) | (_, Err(e)) => {
                first_err.get_or_insert(e);
            }
        }
    }
    (pairs, first_err)
}

fn query_get(pairs: &[(String, String)], key: &str) -> String {
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

fn random_bytes(n: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; n];
    getrandom::fill(&mut buf).map_err(|e| format!("failed to generate random state: {e}"))?;
    Ok(buf)
}

/// `utils.RandomStage`: 128 random bytes, padded URL-safe base64.
pub fn random_state() -> Result<String, String> {
    Ok(gobase64::url_encode(&random_bytes(128)?))
}

/// `oauth2.GenerateVerifier`.
pub fn generate_verifier() -> Result<String, String> {
    Ok(gobase64::raw_url_encode(&random_bytes(32)?))
}

pub fn s256_challenge(verifier: &str) -> String {
    gobase64::raw_url_encode(
        ring::digest::digest(&ring::digest::SHA256, verifier.as_bytes()).as_ref(),
    )
}

/// Compares without an early exit, so response timing does not reveal how
/// much of a forged state matched.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let diff = a
        .iter()
        .zip(b)
        .fold(0u8, |acc, (x, y)| acc | std::hint::black_box(x ^ y));
    std::hint::black_box(diff) == 0
}

pub type OpenUrl = Box<dyn Fn(&str) -> io::Result<()> + Send + Sync>;

/// Where the flow sends its hints and reads a pasted code.
pub struct Prompt {
    pub open_url: OpenUrl,
    pub out: Box<dyn Write + Send>,
    /// `None` never reads a pasted code and waits only for the callback.
    pub input: Option<Box<dyn Read + Send>>,
}

impl Prompt {
    /// The terminal: stdout and stdin, or stderr only under stdio MCP.
    pub fn terminal() -> Prompt {
        if mcp_stdio() {
            Prompt {
                open_url: Box::new(open_browser),
                out: Box::new(io::stderr()),
                input: None,
            }
        } else {
            Prompt {
                open_url: Box::new(open_browser),
                out: Box::new(io::stdout()),
                input: Some(Box::new(io::stdin())),
            }
        }
    }
}

/// Starts the platform's URL opener without waiting for it, as Go's
/// `exec.Command(...).Start()` does.
pub fn open_browser(url: &str) -> io::Result<()> {
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = std::process::Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", url]);
        c
    } else if cfg!(target_os = "linux") {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    } else {
        return Err(io::Error::other(format!(
            "cannot open URL {url} on this platform"
        )));
    };
    let mut child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// `time.Duration.String`.
pub fn go_duration(d: Duration) -> String {
    let ns = d.as_nanos();
    if ns == 0 {
        return "0s".to_string();
    }
    let frac = |v: u128, digits: u32| -> String {
        let unit = 10u128.pow(digits);
        let f = format!("{:0width$}", v % unit, width = digits as usize);
        let f = f.trim_end_matches('0');
        if f.is_empty() {
            format!("{}", v / unit)
        } else {
            format!("{}.{f}", v / unit)
        }
    };
    if ns < 1_000 {
        return format!("{ns}ns");
    }
    if ns < 1_000_000 {
        return format!("{}µs", frac(ns, 3));
    }
    if ns < 1_000_000_000 {
        return format!("{}ms", frac(ns, 6));
    }
    let secs = ns / 1_000_000_000;
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    let mut out = String::new();
    if h > 0 {
        out.push_str(&format!("{h}h"));
    }
    if h > 0 || m > 0 {
        out.push_str(&format!("{m}m"));
    }
    out.push_str(&frac(s * 1_000_000_000 + ns % 1_000_000_000, 9));
    out.push('s');
    out
}

/// The loopback listener that receives the authorization code.
struct CallbackServer {
    codes: mpsc::Receiver<String>,
    accept: tokio::task::JoinHandle<()>,
}

impl Drop for CallbackServer {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

struct Callback {
    state: String,
    codes: mpsc::Sender<String>,
    received: Notify,
}

impl CallbackServer {
    async fn start(redirect_url: &str, state: String) -> Result<CallbackServer, String> {
        let host = redirect_host(redirect_url);
        let listener = TcpListener::bind(host).await.map_err(|e| {
            let text = match e.raw_os_error() {
                Some(code) => root::go_errno_text(code),
                None => e.to_string(),
            };
            format!("failed to start web server: listen tcp {host}: bind: {text}")
        })?;
        let (tx, rx) = mpsc::channel(1);
        let shared = Arc::new(Callback {
            state,
            codes: tx,
            received: Notify::new(),
        });
        let accept = tokio::spawn(async move {
            loop {
                tokio::select! {
                    conn = listener.accept() => {
                        if let Ok((stream, _)) = conn {
                            tokio::spawn(serve(stream, shared.clone()));
                        }
                    }
                    () = shared.received.notified() => return,
                }
            }
        });
        Ok(CallbackServer { codes: rx, accept })
    }

    async fn wait(&mut self, timeout: Duration) -> Result<String, String> {
        match tokio::time::timeout(timeout, self.codes.recv()).await {
            Ok(Some(code)) => Ok(code),
            _ => Err(format!(
                "authorization timed out after {}",
                go_duration(timeout)
            )),
        }
    }
}

/// The `host:port` Go's `url.Parse(redirectURL).Host` yields.
fn redirect_host(url: &str) -> &str {
    let rest = url.split_once("://").map_or("", |(_, rest)| rest);
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    &rest[..end]
}

async fn serve(mut stream: TcpStream, cb: Arc<Callback>) {
    let mut head = Vec::new();
    let read = tokio::time::timeout(REQUEST_HEAD_TIMEOUT, async {
        let mut buf = [0u8; 1024];
        while !head.windows(4).any(|w| w == b"\r\n\r\n") && head.len() < MAX_REQUEST_HEAD {
            let n = stream.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            head.extend_from_slice(&buf[..n]);
        }
        io::Result::Ok(())
    })
    .await;
    if !matches!(read, Ok(Ok(()))) {
        return;
    }
    let line = head.split(|&b| b == b'\n').next().unwrap_or_default();
    let line = String::from_utf8_lossy(line);
    let target = line.split(' ').nth(1).unwrap_or_default();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let response = if path != "/" {
        http_error(404, "Not Found", "404 page not found")
    } else {
        let (pairs, _) = parse_query(query);
        let state = query_get(&pairs, "state");
        if !constant_time_eq(state.as_bytes(), cb.state.as_bytes()) {
            log::error(STATE_MISMATCH, &[]);
            http_error(400, "Bad Request", STATE_MISMATCH)
        } else {
            let code = query_get(&pairs, "code");
            let body = format!(
                "Authorization code received: {code}\nYou can now safely close the browser window.\n"
            );
            match cb.codes.try_send(code) {
                Ok(()) => {
                    cb.received.notify_one();
                    http_response(200, "OK", "text/plain", false, &body)
                }
                Err(_) => http_error(409, "Conflict", CODE_ALREADY_RECEIVED),
            }
        }
    };
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

/// `http.Error`.
fn http_error(code: u16, reason: &str, msg: &str) -> String {
    http_response(
        code,
        reason,
        "text/plain; charset=utf-8",
        true,
        &format!("{msg}\n"),
    )
}

fn http_response(code: u16, reason: &str, content_type: &str, nosniff: bool, body: &str) -> String {
    let nosniff = if nosniff {
        "X-Content-Type-Options: nosniff\r\n"
    } else {
        ""
    };
    format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {content_type}\r\n{nosniff}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// `fmt.Fscan(in, &code)`: the first whitespace-delimited word.
fn read_word(mut input: Box<dyn Read + Send>) -> Result<String, String> {
    let mut word = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match input.read(&mut byte) {
            Ok(0) => break,
            Ok(_) if byte[0].is_ascii_whitespace() || byte[0] == b'\x0b' => {
                if !word.is_empty() {
                    break;
                }
            }
            Ok(_) => word.push(byte[0]),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    if word.is_empty() {
        return Err("EOF".to_string());
    }
    Ok(String::from_utf8_lossy(&word).into_owned())
}

/// `getTokenFromWeb`: consent in the browser, the code from the callback or
/// the prompt, then the PKCE exchange.
pub async fn token_from_web(
    config: &ClientConfig,
    timeout: Duration,
    prompt: Prompt,
) -> Result<Token, String> {
    let state = random_state()?;
    let verifier = generate_verifier()?;
    let auth_url = config.auth_code_url(&state, &verifier);
    let mut server = CallbackServer::start(&config.redirect_url, state).await?;
    let Prompt {
        open_url,
        mut out,
        input,
    } = prompt;

    let mut code = String::new();
    if open_url(&auth_url).is_ok() {
        let _ = out.write_all(browser_opened_hint(&auth_url).as_bytes());
        let _ = out.flush();
        code = server.wait(timeout).await?;
    }
    if code.is_empty() {
        let _ = out.write_all(open_browser_hint(&auth_url).as_bytes());
        let _ = out.write_all(manual_input_hint(&config.redirect_url).as_bytes());
        let _ = out.flush();
        code = match input {
            Some(input) => tokio::task::spawn_blocking(move || read_word(input))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| format!("failed to read prompt: {e}"))?,
            None => server.wait(timeout).await?,
        };
        if let Some(rest) = code.strip_prefix("4%2F") {
            code = format!("4/{rest}");
        }
    }
    drop(server);

    log::debug("Authorization code received", &[]);
    exchange(config, &code, &verifier)
        .await
        .map_err(|e| format!("failed to exchange token: {e}"))
}

/// `Config.Exchange(ctx, code, VerifierOption(verifier))`.
pub async fn exchange(config: &ClientConfig, code: &str, verifier: &str) -> Result<Token, String> {
    let mut v = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
    ];
    if !config.redirect_url.is_empty() {
        v.push(("redirect_uri", config.redirect_url.clone()));
    }
    v.push(("code_verifier", verifier.to_string()));
    retrieve_token(config, v).await
}

/// oauth2's `tokenRefresher.Token`.
pub async fn refresh(config: &ClientConfig, refresh_token: &str) -> Result<Token, String> {
    if refresh_token.is_empty() {
        return Err("oauth2: token expired and refresh token is not set".to_string());
    }
    retrieve_token(
        config,
        vec![
            ("grant_type", "refresh_token".to_string()),
            ("refresh_token", refresh_token.to_string()),
        ],
    )
    .await
}

/// `internal.RetrieveToken`: basic auth first, then form parameters, and
/// the style that worked is kept for this config.
async fn retrieve_token(
    config: &ClientConfig,
    v: Vec<(&'static str, String)>,
) -> Result<Token, String> {
    let known = *config.in_params.lock().expect("auth style lock");
    let mut result = round_trip(config, &v, known.unwrap_or(false)).await;
    if known.is_none() {
        let mut in_params = false;
        if result.is_err() {
            in_params = true;
            result = round_trip(config, &v, true).await;
        }
        if result.is_ok() {
            *config.in_params.lock().expect("auth style lock") = Some(in_params);
        }
    }
    let mut token = result?;
    if token.refresh_token.is_empty() {
        token.refresh_token = query_get(
            &v.iter()
                .map(|(k, val)| (k.to_string(), val.clone()))
                .collect::<Vec<_>>(),
            "refresh_token",
        );
    }
    Ok(token)
}

/// `internal.doTokenRoundTrip`.
async fn round_trip(
    config: &ClientConfig,
    params: &[(&'static str, String)],
    in_params: bool,
) -> Result<Token, String> {
    if !secure_token_url(&config.token_url) {
        return Err(format!(
            "refusing to send credentials to {}: the token endpoint must use https",
            log::quote(&config.token_url)
        ));
    }
    let mut v = params.to_vec();
    if in_params {
        if !config.client_id.is_empty() {
            set(&mut v, "client_id", config.client_id.clone());
        }
        if !config.client_secret.is_empty() {
            set(&mut v, "client_secret", config.client_secret.clone());
        }
    }
    let mut req = yutu_api::client::http_client()
        .post(&config.token_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(encode_form(v));
    if !in_params {
        let pair = format!(
            "{}:{}",
            query_escape(&config.client_id),
            query_escape(&config.client_secret)
        );
        req = req.header(
            "Authorization",
            format!("Basic {}", gobase64::std_encode(pair.as_bytes())),
        );
    }
    let post_err = |e: reqwest::Error| format!("Post {}: {e}", log::quote(&config.token_url));
    let mut resp = req.send().await.map_err(post_err)?;
    let status = resp.status();
    let content_type = resp
        .headers()
        .get("Content-Type")
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let mut body = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("oauth2: cannot fetch token: {e}"))?
    {
        body.extend_from_slice(&chunk);
        if body.len() >= MAX_TOKEN_BODY {
            body.truncate(MAX_TOKEN_BODY);
            break;
        }
    }
    let failure = !status.is_success();
    let mut err = RetrieveError {
        status: format!(
            "{} {}",
            status.as_u16(),
            status.canonical_reason().unwrap_or_default()
        ),
        body: String::from_utf8_lossy(&body).into_owned(),
        code: String::new(),
        description: String::new(),
        uri: String::new(),
    };

    let token =
        if content_type == "application/x-www-form-urlencoded" || content_type == "text/plain" {
            let (vals, parse_err) = parse_query(&String::from_utf8_lossy(&body));
            if let Some(e) = parse_err {
                return Err(if failure {
                    err.to_string()
                } else {
                    format!("oauth2: cannot parse response: {e}")
                });
            }
            err.code = query_get(&vals, "error");
            err.description = query_get(&vals, "error_description");
            err.uri = query_get(&vals, "error_uri");
            let expires = query_get(&vals, "expires_in").parse::<i64>().unwrap_or(0);
            Token {
                access_token: query_get(&vals, "access_token"),
                token_type: query_get(&vals, "token_type"),
                refresh_token: query_get(&vals, "refresh_token"),
                expiry: if expires != 0 {
                    GoTime::now_plus(expires)
                } else {
                    GoTime::ZERO
                },
                expires_in: 0,
            }
        } else {
            let tj = match TokenJson::parse(&body) {
                Ok(tj) => tj,
                Err(e) => {
                    return Err(if failure {
                        err.to_string()
                    } else {
                        format!("oauth2: cannot parse json: {e}")
                    });
                }
            };
            err.code = tj.error;
            err.description = tj.error_description;
            err.uri = tj.error_uri;
            Token {
                access_token: tj.access_token,
                token_type: tj.token_type,
                refresh_token: tj.refresh_token,
                expiry: if tj.expires_in != 0 {
                    GoTime::now_plus(tj.expires_in)
                } else {
                    GoTime::ZERO
                },
                expires_in: tj.expires_in,
            }
        };
    if failure || !err.code.is_empty() {
        return Err(err.to_string());
    }
    if token.access_token.is_empty() {
        return Err("oauth2: server response missing access_token".to_string());
    }
    Ok(token)
}

/// The client secret, code and refresh token only go to an https token
/// endpoint, or plain http on loopback for local testing.
fn secure_token_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") {
        return true;
    }
    let Some(rest) = lower.strip_prefix("http://") else {
        return false;
    };
    let auth = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    if auth.contains(['@', '\\']) {
        return false;
    }
    let host = auth.rsplit_once(':').map(|(h, _)| h).unwrap_or(auth);
    matches!(host, "127.0.0.1" | "localhost" | "[::1]")
}

/// oauth2's `RetrieveError`.
struct RetrieveError {
    status: String,
    body: String,
    code: String,
    description: String,
    uri: String,
}

impl fmt::Display for RetrieveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.code.is_empty() {
            return write!(
                f,
                "oauth2: cannot fetch token: {}\nResponse: {}",
                self.status, self.body
            );
        }
        write!(f, "oauth2: {}", log::quote(&self.code))?;
        if !self.description.is_empty() {
            write!(f, " {}", log::quote(&self.description))?;
        }
        if !self.uri.is_empty() {
            write!(f, " {}", log::quote(&self.uri))?;
        }
        Ok(())
    }
}

/// oauth2's internal `tokenJSON`.
#[derive(Default)]
struct TokenJson {
    access_token: String,
    token_type: String,
    refresh_token: String,
    expires_in: i64,
    error: String,
    error_description: String,
    error_uri: String,
}

impl TokenJson {
    fn parse(body: &[u8]) -> Result<TokenJson, String> {
        let value: Value = serde_json::from_slice(body).map_err(|e| e.to_string())?;
        let map = match value {
            Value::Null => return Ok(TokenJson::default()),
            Value::Object(m) => m,
            other => {
                return Err(format!(
                    "json: cannot unmarshal {} into Go value of type internal.tokenJSON",
                    kind(&other)
                ));
            }
        };
        let text = |name: &str| -> Result<String, String> {
            match field(&map, name) {
                None | Some(Value::Null) => Ok(String::new()),
                Some(Value::String(s)) => Ok(s.clone()),
                Some(other) => Err(format!(
                    "json: cannot unmarshal {} into Go struct field tokenJSON.{name} of type string",
                    kind(other)
                )),
            }
        };
        // Some servers send expires_in as a string; oauth2 accepts both and
        // caps the value at MaxInt32.
        let expires_in = match field(&map, "expires_in") {
            None | Some(Value::Null) => 0,
            Some(Value::Number(n)) => n.as_i64().ok_or_else(|| {
                format!(
                    "strconv.ParseInt: parsing {}: invalid syntax",
                    log::quote(&n.to_string())
                )
            })?,
            Some(Value::String(s)) => s.parse::<i64>().map_err(|_| {
                format!(
                    "strconv.ParseInt: parsing {}: invalid syntax",
                    log::quote(s)
                )
            })?,
            Some(other) => {
                return Err(format!(
                    "json: cannot unmarshal {} into Go struct field tokenJSON.expires_in",
                    kind(other)
                ));
            }
        };
        Ok(TokenJson {
            access_token: text("access_token")?,
            token_type: text("token_type")?,
            refresh_token: text("refresh_token")?,
            expires_in: i64::from(i32::try_from(expires_in).unwrap_or(if expires_in > 0 {
                i32::MAX
            } else {
                expires_in as i32
            })),
            error: text("error")?,
            error_description: text("error_description")?,
            error_uri: text("error_uri")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = br#"{"installed":{"client_id":"cid.apps.googleusercontent.com","client_secret":"sec","redirect_uris":["http://localhost"],"auth_uri":"https://accounts.google.com/o/oauth2/auth","token_uri":"https://oauth2.googleapis.com/token"}}"#;

    #[test]
    fn auth_url_matches_go_oauth2() {
        let mut config = ClientConfig::from_json(SECRET).unwrap();
        assert_eq!(config.redirect_url, "http://localhost");
        config.redirect_url = DEFAULT_REDIRECT_URL.to_string();
        // Produced by Go oauth2 v0.37.0 AuthCodeURL with the same inputs.
        assert_eq!(
            config.auth_code_url("st+/=", "verifier-abc"),
            "https://accounts.google.com/o/oauth2/auth?access_type=offline&client_id=cid.apps.googleusercontent.com&code_challenge=K5x6-41mFxTvpObzIPEdq05UvvzIql6sRCB8gV2ba04&code_challenge_method=S256&prompt=consent&redirect_uri=http%3A%2F%2Flocalhost%3A8216&response_type=code&scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fyoutube+https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fyoutube.force-ssl+https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fyoutube.channel-memberships.creator&state=st%2B%2F%3D"
        );
    }

    #[test]
    fn config_errors_match_go() {
        assert_eq!(
            ClientConfig::from_json(b"{}").unwrap_err(),
            "oauth2/google: no credentials found"
        );
        assert_eq!(
            ClientConfig::from_json(br#"{"web":{"client_id":"x"}}"#).unwrap_err(),
            "oauth2/google: missing redirect URL in the client_credentials.json"
        );
        let web = ClientConfig::from_json(
            br#"{"installed":{"redirect_uris":["a"]},"WEB":{"client_id":"w","redirect_uris":["b"]}}"#,
        )
        .unwrap();
        assert_eq!(
            (web.client_id.as_str(), web.redirect_url.as_str()),
            ("w", "b")
        );
    }

    #[test]
    fn state_and_verifier_shapes() {
        let state = random_state().unwrap();
        assert_eq!(state.len(), 172);
        assert!(state.ends_with('='));
        assert_eq!(generate_verifier().unwrap().len(), 43);
    }

    #[test]
    fn durations_print_like_go() {
        assert_eq!(go_duration(CALLBACK_TIMEOUT), "2m0s");
        assert_eq!(go_duration(Duration::from_millis(300)), "300ms");
        assert_eq!(go_duration(Duration::from_millis(1500)), "1.5s");
        assert_eq!(go_duration(Duration::from_secs(3600)), "1h0m0s");
    }

    #[test]
    fn query_parsing() {
        let (pairs, err) = parse_query("state=a%2Bb+c&code=4%2Fx&bad=%zz&x;y=1");
        assert_eq!(
            pairs,
            vec![
                ("state".to_string(), "a+b c".to_string()),
                ("code".to_string(), "4/x".to_string())
            ]
        );
        assert_eq!(err.as_deref(), Some("invalid URL escape \"%zz\""));
    }

    #[test]
    fn word_reading_matches_fscan() {
        let read = |s: &'static str| read_word(Box::new(s.as_bytes()));
        assert_eq!(read(""), Err("EOF".to_string()));
        assert_eq!(read("   \n"), Err("EOF".to_string()));
        assert_eq!(read("  4%2Fabc def\n"), Ok("4%2Fabc".to_string()));
        assert_eq!(read("\n\ncode"), Ok("code".to_string()));
    }

    #[test]
    fn token_endpoint_must_be_secure() {
        assert!(secure_token_url("https://oauth2.googleapis.com/token"));
        assert!(secure_token_url("http://127.0.0.1:5000/token"));
        assert!(!secure_token_url("http://oauth2.googleapis.com/token"));
        assert!(!secure_token_url("http://localhost:80@evil.example/token"));
    }

    #[test]
    fn constant_time_compare() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }
}
