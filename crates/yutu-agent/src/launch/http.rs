//! The HTTP/1.1 server under every web launcher: Go `http.Server`'s
//! timeouts and body cap, `net/http`'s plain error and redirect bodies, and
//! the request log line ADK's middleware writes.

use bytes::Bytes;
use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use http_body_util::{BodyExt, Full, Limited, StreamBody, combinators::BoxBody};
use hyper::body::{Frame, Incoming};
use hyper_util::rt::{TokioIo, TokioTimer};
use serde_json::Value;
use std::convert::Infallible;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};

/// A request with its body already read under the cap.
pub struct Req {
    pub method: Method,
    /// The percent-decoded path, as Go's `URL.Path`.
    pub path: String,
    /// The path and query as sent, Go's `RequestURI`.
    pub uri: String,
    pub query: String,
    pub headers: HeaderMap,
    /// `None` when the body went past the cap: handlers that read it report
    /// Go's `http: request body too large`.
    pub body: Option<Bytes>,
    /// The address the connection was accepted on.
    pub local: SocketAddr,
    pub start: Instant,
}

pub const TOO_LARGE: &str = "http: request body too large";

impl Req {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    /// The first value of a query parameter, percent-decoded.
    pub fn param(&self, name: &str) -> Option<String> {
        self.query.split('&').find_map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (decode(k) == name).then(|| decode(v))
        })
    }

    pub fn body(&self) -> Result<&[u8], &'static str> {
        self.body.as_deref().ok_or(TOO_LARGE)
    }
}

pub enum Body {
    Full(Bytes),
    /// Chunks written and flushed as they arrive, until the sender drops or
    /// the deadline passes.
    Stream(mpsc::UnboundedReceiver<Bytes>, Option<Instant>),
}

pub struct Resp {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Body,
}

impl Resp {
    pub fn new(status: u16) -> Resp {
        Resp {
            status,
            headers: Vec::new(),
            body: Body::Full(Bytes::new()),
        }
    }

    pub fn header(mut self, name: &'static str, value: impl Into<String>) -> Resp {
        self.headers.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        self.headers.push((name, value.into()));
        self
    }

    pub fn body(mut self, body: impl Into<Bytes>) -> Resp {
        self.body = Body::Full(body.into());
        self
    }

    /// ADK's `EncodeJSONResponse`.
    pub fn json(status: u16, v: &Value) -> Resp {
        let mut s = crate::gojson::encode(v);
        s.push('\n');
        Resp::new(status)
            .header("content-type", "application/json; charset=UTF-8")
            .body(s)
    }

    /// `http.Error`.
    pub fn error(status: u16, msg: &str) -> Resp {
        Resp::new(status)
            .header("content-type", "text/plain; charset=utf-8")
            .header("x-content-type-options", "nosniff")
            .body(format!("{msg}\n"))
    }

    pub fn not_found() -> Resp {
        Resp::error(404, "404 page not found")
    }

    /// `http.Redirect` for a GET or HEAD.
    pub fn redirect(status: u16, location: &str) -> Resp {
        let text = StatusCode::from_u16(status)
            .ok()
            .and_then(|s| s.canonical_reason())
            .unwrap_or_default();
        Resp::new(status)
            .header("location", location)
            .header("content-type", "text/html; charset=utf-8")
            .body(format!(
                "<a href=\"{}\">{text}</a>.\n\n",
                html_escape(location)
            ))
    }

    pub fn stream(
        status: u16,
        rx: mpsc::UnboundedReceiver<Bytes>,
        deadline: Option<Instant>,
    ) -> Resp {
        Resp {
            status,
            headers: Vec::new(),
            body: Body::Stream(rx, deadline),
        }
    }
}

/// Go's `url.PathUnescape`, leaving invalid escapes as they are.
pub fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && b[i + 1].is_ascii_hexdigit()
            && b[i + 2].is_ascii_hexdigit()
        {
            let hex = |c: u8| (c as char).to_digit(16).unwrap_or(0) as u8;
            out.push(hex(b[i + 1]) * 16 + hex(b[i + 2]));
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[derive(Clone, Copy)]
pub struct Limits {
    pub read: Option<Duration>,
    pub write: Option<Duration>,
    pub idle: Option<Duration>,
    pub body: u64,
}

type HttpBody = BoxBody<Bytes, std::io::Error>;

/// Serves `handler` on `listener` until `shutdown` resolves, then lets open
/// connections finish for up to `grace`.
pub async fn serve<H, F>(
    listener: TcpListener,
    limits: Limits,
    handler: H,
    shutdown: impl Future<Output = ()>,
    grace: Option<Duration>,
) where
    H: Fn(Req) -> F + Send + Sync + 'static,
    F: Future<Output = Resp> + Send + 'static,
{
    let handler = Arc::new(handler);
    let (stop_tx, stop_rx) = watch::channel(false);
    let mut conns = tokio::task::JoinSet::new();
    tokio::pin!(shutdown);
    loop {
        let (stream, local) = tokio::select! {
            () = &mut shutdown => break,
            accepted = listener.accept() => match accepted {
                Ok((s, _)) => {
                    let local = s.local_addr().unwrap_or_else(|_| ([127, 0, 0, 1], 0).into());
                    (s, local)
                }
                Err(_) => {
                    // Accept errors (out of descriptors, mostly) persist
                    // until connections close; retrying at once would spin.
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            },
        };
        while conns.try_join_next().is_some() {}
        let handler = handler.clone();
        let mut stop = stop_rx.clone();
        conns.spawn(async move {
            let svc = hyper::service::service_fn(move |req: hyper::Request<Incoming>| {
                let handler = handler.clone();
                async move { respond(req, local, limits, handler.as_ref()).await }
            });
            let mut builder = hyper::server::conn::http1::Builder::new();
            builder.timer(TokioTimer::new());
            // hyper's header timer also runs while a kept-alive connection
            // waits for its next request, so it bounds both.
            if let Some(t) = [limits.read, limits.idle].into_iter().flatten().min() {
                builder.header_read_timeout(t);
            }
            let conn = builder.serve_connection(TokioIo::new(stream), svc);
            tokio::pin!(conn);
            tokio::select! {
                _ = conn.as_mut() => {}
                () = async { let _ = stop.wait_for(|s| *s).await; } => {
                    conn.as_mut().graceful_shutdown();
                    let _ = conn.await;
                }
            }
        });
    }
    drop(listener);
    let _ = stop_tx.send(true);
    let drain = async { while conns.join_next().await.is_some() {} };
    match grace {
        Some(g) => {
            let _ = tokio::time::timeout(g, drain).await;
        }
        None => drain.await,
    }
}

async fn respond<H, F>(
    req: hyper::Request<Incoming>,
    local: SocketAddr,
    limits: Limits,
    handler: &H,
) -> Result<hyper::Response<HttpBody>, std::io::Error>
where
    H: Fn(Req) -> F,
    F: Future<Output = Resp>,
{
    let start = Instant::now();
    let (parts, body) = req.into_parts();
    let collect = Limited::new(body, limits.body as usize).collect();
    let body = match limits.read {
        Some(t) => tokio::time::timeout(t, collect)
            .await
            .map_err(|_| std::io::Error::other("read timeout"))?,
        None => collect.await,
    };
    let body = match body {
        Ok(c) => Some(c.to_bytes()),
        Err(e)
            if e.downcast_ref::<http_body_util::LengthLimitError>()
                .is_some() =>
        {
            None
        }
        Err(e) => return Err(std::io::Error::other(e.to_string())),
    };
    let uri = parts
        .uri
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| "/".into());
    let req = Req {
        method: parts.method.clone(),
        path: decode(parts.uri.path()),
        uri: uri.clone(),
        query: parts.uri.query().unwrap_or_default().to_string(),
        headers: parts.headers,
        body,
        local,
        start,
    };
    let resp = match limits.write {
        Some(t) => tokio::time::timeout(t, handler(req))
            .await
            .map_err(|_| std::io::Error::other("write timeout"))?,
        None => handler(req).await,
    };
    yutu_auth::log::info(
        &format!(
            "{} {} {}",
            parts.method,
            redact_token(&uri.to_string()),
            yutu_auth::flow::go_duration(start.elapsed())
        ),
        &[],
    );
    let mut builder = hyper::Response::builder().status(resp.status);
    for (name, value) in &resp.headers {
        if let (Ok(n), Ok(v)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            builder = builder.header(n, v);
        }
    }
    let body: HttpBody = match resp.body {
        Body::Full(b) if parts.method == Method::HEAD => {
            let len = b.len();
            builder = builder.header("content-length", len);
            Full::new(Bytes::new())
                .map_err(|e: Infallible| match e {})
                .boxed()
        }
        Body::Full(b) => Full::new(b).map_err(|e: Infallible| match e {}).boxed(),
        Body::Stream(rx, deadline) => {
            let deadline = deadline.map(tokio::time::Instant::from_std);
            let frames = futures_util::stream::unfold(Some(rx), move |rx| async move {
                let mut rx = rx?;
                let next = match deadline {
                    Some(d) => match tokio::time::timeout_at(d, rx.recv()).await {
                        Ok(chunk) => chunk.map(Ok),
                        // Go's write deadline: the connection is cut.
                        Err(_) => return Some((Err(std::io::Error::other("write timeout")), None)),
                    },
                    None => rx.recv().await.map(Ok),
                };
                next.map(|r| (r.map(Frame::data), Some(rx)))
            });
            StreamBody::new(frames).boxed()
        }
    };
    builder
        .body(body)
        .map_err(|e| std::io::Error::other(e.to_string()))
}

/// The request target with any `token` value hidden, for the access log.
fn redact_token(target: &str) -> String {
    let Some((path, query)) = target.split_once('?') else {
        return target.to_string();
    };
    let query: Vec<String> = query
        .split('&')
        .map(|p| {
            if is_token_param(p) {
                "token=REDACTED".to_string()
            } else {
                p.to_string()
            }
        })
        .collect();
    format!("{path}?{}", query.join("&"))
}

/// Whether a query pair's name is `token`, however it is percent-encoded.
pub fn is_token_param(pair: &str) -> bool {
    let name = pair.split_once('=').map_or(pair, |(k, _)| k);
    decode(name) == "token"
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod redact_tests {
    #[test]
    fn the_access_log_hides_tokens() {
        assert_eq!(
            super::redact_token("/ui/?mode=x&token=secret&b=1"),
            "/ui/?mode=x&token=REDACTED&b=1"
        );
        assert_eq!(
            super::redact_token("/ui/?%74oken=secret"),
            "/ui/?token=REDACTED"
        );
        assert_eq!(super::redact_token("/api/list-apps"), "/api/list-apps");
    }
}
