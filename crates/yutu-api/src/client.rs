//! The YouTube Data API client: one shared reqwest client over rustls, with
//! the same query parameters, bodies and media framing as the Go client.

use crate::error::{ApiError, Error};
use crate::schema::{Location, Method, method};
use crate::sniff::detect_content_type;
use crate::value::Value;
use crate::{decode, json};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

pub const DEFAULT_ENDPOINT: &str = "https://youtube.googleapis.com/";
/// googleapi.DefaultUploadChunkSize: media at or under this size goes in one
/// multipart request, as Go sends it.
pub const SINGLE_REQUEST_LIMIT: u64 = 16 << 20;
/// Chunk size for resumable uploads of larger media (divergence E7).
pub const RESUMABLE_CHUNK: u64 = 8 << 20;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Supplies a bearer token for each request, refreshing as needed.
pub trait TokenSource: Send + Sync {
    fn token(&self) -> BoxFuture<'_, Result<String, String>>;
}

#[derive(Clone)]
pub struct Service {
    http: reqwest::Client,
    base: String,
    tokens: Option<Arc<dyn TokenSource>>,
}

/// The API base URL. With the `parity-endpoint` feature, `YUTU_PARITY_ENDPOINT`
/// points requests at the local parity recorder instead.
pub fn endpoint() -> String {
    #[cfg(feature = "parity-endpoint")]
    if let Ok(e) = std::env::var("YUTU_PARITY_ENDPOINT") {
        if !e.is_empty() {
            return format!("{}/", e.trim_end_matches('/'));
        }
    }
    DEFAULT_ENDPOINT.to_string()
}

/// Whether requests go to the parity recorder, which needs no credentials.
pub fn parity_mode() -> bool {
    endpoint() != DEFAULT_ENDPOINT
}

pub fn http_client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(60))
        .user_agent(concat!("yutu-rs/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("TLS backend available")
}

impl Service {
    pub fn new(tokens: Option<Arc<dyn TokenSource>>) -> Service {
        Service {
            http: http_client(),
            base: endpoint(),
            tokens,
        }
    }

    pub fn with_base(base: impl Into<String>, tokens: Option<Arc<dyn TokenSource>>) -> Service {
        Service {
            http: http_client(),
            base: base.into(),
            tokens,
        }
    }

    pub fn call(&self, method_id: &str) -> Call<'_> {
        Call {
            svc: self,
            method: method(method_id),
            query: Vec::new(),
            path: Vec::new(),
            body: None,
            media: None,
        }
    }
}

pub enum Media {
    Bytes(Vec<u8>),
    File(std::path::PathBuf),
}

pub struct Call<'a> {
    svc: &'a Service,
    method: &'static Method,
    query: Vec<(String, String)>,
    path: Vec<(String, String)>,
    body: Option<String>,
    media: Option<Media>,
}

impl Call<'_> {
    /// Sets a parameter the way the generated Go setter does: path params fill
    /// the URL, query params replace any earlier value.
    pub fn param(mut self, name: &str, value: impl ToString) -> Self {
        let value = value.to_string();
        match self
            .method
            .params
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.location)
        {
            Some(Location::Path) => self.path.push((name.to_string(), value)),
            _ => {
                self.query.retain(|(k, _)| k != name);
                self.query.push((name.to_string(), value));
            }
        }
        self
    }

    /// A repeated query parameter, like Go's `SetMulti`.
    pub fn params(mut self, name: &str, values: &[String]) -> Self {
        self.query.retain(|(k, _)| k != name);
        for v in values {
            self.query.push((name.to_string(), v.clone()));
        }
        self
    }

    /// The JSON request body, encoded as gensupport would with a trailing
    /// newline from json.Encoder.
    pub fn body(mut self, v: &Value) -> Self {
        self.body = Some(format!("{}\n", json::body(v)));
        self
    }

    pub fn media(mut self, media: Media) -> Self {
        self.media = Some(media);
        self
    }

    /// The HTTP method and URL `download()` would send.
    pub fn preview_download(&self) -> (&'static str, String) {
        (self.method.http_method, self.url(self.method.path, "media", None))
    }

    /// The HTTP method and URL this call would send, for tests and lanes.
    pub fn preview(&self) -> (&'static str, String) {
        match (&self.media, self.method.upload_path) {
            (Some(_), Some(p)) => (
                self.method.http_method,
                self.url(p, "json", Some("multipart")),
            ),
            _ => (
                self.method.http_method,
                self.url(self.method.path, "json", None),
            ),
        }
    }

    pub fn body_text(&self) -> Option<&str> {
        self.body.as_deref()
    }

    fn url(&self, path: &str, alt: &str, upload_type: Option<&str>) -> String {
        let mut path = path.to_string();
        for (k, v) in &self.path {
            path = path.replace(&format!("{{{k}}}"), &encode_path(v));
        }
        let mut query: Vec<(&str, &str)> = self
            .query
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        query.push(("alt", alt));
        query.push(("prettyPrint", "false"));
        if let Some(t) = upload_type {
            query.push(("uploadType", t));
        }
        query.sort_by(|a, b| a.0.cmp(b.0));
        let q: Vec<String> = query
            .iter()
            .map(|(k, v)| format!("{}={}", encode_query(k), encode_query(v)))
            .collect();
        format!("{}{path}?{}", self.svc.base, q.join("&"))
    }

    async fn request(
        &self,
        http_method: &str,
        url: &str,
    ) -> Result<reqwest::RequestBuilder, Error> {
        let m = reqwest::Method::from_bytes(http_method.as_bytes())
            .map_err(|e| Error::Local(e.to_string()))?;
        let mut req = self.svc.http.request(m, url);
        if let Some(t) = &self.svc.tokens {
            let token = t.token().await.map_err(Error::Local)?;
            req = req.bearer_auth(token);
        }
        Ok(req)
    }

    async fn execute(self, alt: &str) -> Result<reqwest::Response, Error> {
        let http_method = self.method.http_method;
        let resp = match (&self.media, self.method.upload_path) {
            (Some(_), Some(upload_path)) => return self.upload(upload_path).await,
            _ => {
                let url = self.url(self.method.path, alt, None);
                let mut req = self.request(http_method, &url).await?;
                if let Some(b) = &self.body {
                    req = req
                        .header("Content-Type", "application/json")
                        .body(b.clone());
                }
                req.send()
                    .await
                    .map_err(|e| transport(http_method, &url, e))?
            }
        };
        check(resp).await
    }

    async fn upload(self, upload_path: &'static str) -> Result<reqwest::Response, Error> {
        let media = self.media.as_ref().unwrap();
        let (size, head) = match media {
            Media::Bytes(b) => (b.len() as u64, b[..b.len().min(512)].to_vec()),
            Media::File(p) => {
                use tokio::io::AsyncReadExt;
                let mut f = tokio::fs::File::open(p)
                    .await
                    .map_err(|e| Error::Local(e.to_string()))?;
                let size = f
                    .metadata()
                    .await
                    .map_err(|e| Error::Local(e.to_string()))?
                    .len();
                let mut head = vec![0u8; size.min(512) as usize];
                f.read_exact(&mut head)
                    .await
                    .map_err(|e| Error::Local(e.to_string()))?;
                (size, head)
            }
        };
        let ctype = detect_content_type(&head);
        if size > SINGLE_REQUEST_LIMIT {
            return crate::media::resumable(&self, upload_path, ctype, size).await;
        }
        let bytes = match media {
            Media::Bytes(b) => b.clone(),
            Media::File(p) => tokio::fs::read(p)
                .await
                .map_err(|e| Error::Local(e.to_string()))?,
        };
        let boundary = random_hex(30);
        let mut body = Vec::with_capacity(bytes.len() + 512);
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Type: application/json\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(self.body.as_deref().unwrap_or("").as_bytes());
        body.extend_from_slice(
            format!("\r\n--{boundary}\r\nContent-Type: {ctype}\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(&bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let url = self.url(upload_path, "json", Some("multipart"));
        let req = self.request(self.method.http_method, &url).await?;
        let resp = req
            .header(
                "Content-Type",
                format!("multipart/related; boundary={boundary}"),
            )
            .body(body)
            .send()
            .await
            .map_err(|e| transport(self.method.http_method, &url, e))?;
        check(resp).await
    }

    pub(crate) fn resumable_parts(
        &self,
        upload_path: &str,
    ) -> (String, &'static str, Option<String>, &Media) {
        (
            self.url(upload_path, "json", Some("resumable")),
            self.method.http_method,
            self.body.clone(),
            self.media.as_ref().unwrap(),
        )
    }

    pub(crate) async fn authorized(
        &self,
        http_method: &str,
        url: &str,
    ) -> Result<reqwest::RequestBuilder, Error> {
        self.request(http_method, url).await
    }

    /// Runs the call and decodes the response into the method's Go type,
    /// including the `ServerResponse` status and headers Go keeps.
    pub async fn send(self) -> Result<Value, Error> {
        let ty = self.method.response.expect("method has a response type");
        let resp = self.execute("json").await?;
        let status = resp.status().as_u16();
        let headers = go_headers(resp.headers());
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| Error::Transport(e.to_string()))?;
        let mut v = decode::decode(ty, &bytes).map_err(|e| Error::Decode(e.0))?;
        if let Some(s) = v.as_struct_mut()
            && let Some(sr) = crate::schema::type_def(s.ty)
                .fields
                .iter()
                .position(|f| f.go == "ServerResponse")
            && let Value::Struct(sr) = &mut s.fields[sr]
        {
            sr.fields[0] = Value::Int(status as i64);
            sr.fields[1] = Value::Header(headers);
        }
        Ok(v)
    }

    /// Runs a call whose method has no response body.
    pub async fn send_empty(self) -> Result<(), Error> {
        self.execute("json").await.map(|_| ())
    }

    /// `alt=media` download, as the Go client's `Download` does.
    pub async fn download(self) -> Result<reqwest::Response, Error> {
        self.execute("media").await
    }
}

pub(crate) async fn check(resp: reqwest::Response) -> Result<reqwest::Response, Error> {
    if resp.status().is_success() {
        return Ok(resp);
    }
    let status = resp.status().as_u16();
    let body = resp.bytes().await.unwrap_or_default();
    Err(Error::Api(ApiError::from_body(status, &body)))
}

pub(crate) fn transport(method: &str, url: &str, e: reqwest::Error) -> Error {
    let verb = method[..1].to_string() + &method[1..].to_lowercase();
    let mut cause: &dyn std::error::Error = &e;
    while let Some(next) = cause.source() {
        cause = next;
    }
    Error::Transport(format!("{verb} \"{url}\": {cause}"))
}

/// Header names in Go's canonical MIME form, as `http.Header` keeps them.
fn go_headers(h: &reqwest::header::HeaderMap) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for (k, v) in h {
        let key = canonical_header(k.as_str());
        let val = String::from_utf8_lossy(v.as_bytes()).into_owned();
        match out.iter_mut().find(|(n, _)| *n == key) {
            Some((_, vals)) => vals.push(val),
            None => out.push((key, vec![val])),
        }
    }
    out
}

pub fn canonical_header(k: &str) -> String {
    let mut upper = true;
    k.chars()
        .map(|c| {
            let out = if upper {
                c.to_ascii_uppercase()
            } else {
                c.to_ascii_lowercase()
            };
            upper = c == '-';
            out
        })
        .collect()
}

pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("OS randomness");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

fn encode_query(s: &str) -> String {
    let mut out = String::new();
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

fn encode_path(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
