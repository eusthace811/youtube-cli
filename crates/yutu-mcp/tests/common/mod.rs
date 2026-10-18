//! A local stand-in for Google's tokeninfo and the YouTube API, and a
//! server under test bound to a free loopback port.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use yutu_mcp::http::{Config, HttpServer};

pub const SCOPES: &str = "https://www.googleapis.com/auth/youtube https://www.googleapis.com/auth/youtube.force-ssl https://www.googleapis.com/auth/youtube.channel-memberships.creator";
pub const BASE: &str = "http://localhost:8216";

#[derive(Clone, Debug)]
pub struct Seen {
    pub request_line: String,
    pub head: String,
    pub body: String,
}

/// An HTTP/1.1 endpoint that records each request and answers it with
/// `answer(request_line, body)`.
pub async fn mock(answer: fn(&str, &str) -> (u16, String)) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        while let Ok((mut conn, _)) = listener.accept().await {
            let log = log.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                loop {
                    let n = conn.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf).into_owned();
                    let Some(end) = text.find("\r\n\r\n") else {
                        continue;
                    };
                    let head = text[..end].to_string();
                    let len = head
                        .lines()
                        .find_map(|l| {
                            let (k, v) = l.split_once(':')?;
                            k.eq_ignore_ascii_case("content-length")
                                .then(|| v.trim().parse().ok())?
                        })
                        .unwrap_or(0usize);
                    if buf.len() < end + 4 + len {
                        continue;
                    }
                    let body = text[end + 4..end + 4 + len].to_string();
                    let request_line = head.lines().next().unwrap_or("").to_string();
                    let (status, payload) = answer(&request_line, &body);
                    log.lock().unwrap().push(Seen {
                        request_line,
                        head,
                        body,
                    });
                    let resp = format!(
                        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{payload}",
                        payload.len()
                    );
                    let _ = conn.write_all(resp.as_bytes()).await;
                    return;
                }
            });
        }
    });
    (url, seen)
}

/// Google's tokeninfo for a handful of test tokens, read from the form body.
pub fn tokeninfo(_line: &str, body: &str) -> (u16, String) {
    let token = body.strip_prefix("access_token=").unwrap_or("");
    let info = |scope: &str, aud: &str| {
        serde_json::json!({"aud": aud, "azp": aud, "sub": "1001", "scope": scope, "expires_in": "3599"})
            .to_string()
    };
    match token {
        "good-token" => (200, info(SCOPES, "client-a")),
        "few-scopes" => (
            200,
            info("https://www.googleapis.com/auth/youtube", "client-a"),
        ),
        _ => (400, r#"{"error":"invalid_token"}"#.into()),
    }
}

pub async fn serve(
    tokeninfo_url: String,
    allowed: Option<Vec<String>>,
    api_base: String,
) -> String {
    let server = HttpServer::bind(Config {
        host: "127.0.0.1".into(),
        port: 0,
        base_url: BASE.into(),
        tokeninfo_url,
        allowed_clients: allowed,
        api_base,
        version: "test".into(),
    })
    .await
    .unwrap();
    let url = format!("http://{}/mcp", server.local_addr().unwrap());
    tokio::spawn(server.run());
    url
}

/// A client over the same rustls setup yutu uses.
pub fn client() -> reqwest::Client {
    yutu_api::client::http_client()
}

pub fn mcp_request(
    client: &reqwest::Client,
    url: &str,
    body: impl Into<reqwest::Body>,
) -> reqwest::RequestBuilder {
    client
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("Mcp-Protocol-Version", "2025-06-18")
        .body(body)
}
