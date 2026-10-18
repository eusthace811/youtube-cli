#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// A fresh directory under the system temp dir, canonicalized so it matches
/// what the working directory resolves to.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let mut name = [0u8; 8];
        getrandom::fill(&mut name).unwrap();
        let hex: String = name.iter().map(|b| format!("{b:02x}")).collect();
        let path = std::env::temp_dir().join(format!("yutu-auth-{hex}"));
        std::fs::create_dir(&path).unwrap();
        TempDir(path.canonicalize().unwrap())
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    pub fn str(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Removes the `time=...` field, the only part of a log line that varies.
pub fn without_time(log: &str) -> String {
    log.lines()
        .map(|l| l.split_once(' ').map_or(l, |(_, rest)| rest))
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Clone, Debug)]
pub struct Recorded {
    pub head: String,
    pub body: String,
}

/// A token endpoint that answers each POST with the next canned JSON body
/// and records what it was sent.
pub struct MockTokenEndpoint {
    pub url: String,
    pub requests: Arc<Mutex<Vec<Recorded>>>,
}

impl MockTokenEndpoint {
    pub async fn start(responses: Vec<&'static str>) -> MockTokenEndpoint {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/token", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        tokio::spawn(async move {
            let mut responses = responses.into_iter();
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let mut data = Vec::new();
                let mut buf = [0u8; 4096];
                let (head, body) = loop {
                    let n = stream.read(&mut buf).await.unwrap();
                    data.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&data).into_owned();
                    if let Some((head, body)) = text.split_once("\r\n\r\n") {
                        let len = head
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length: ")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if body.len() >= len || n == 0 {
                            break (head.to_string(), body.to_string());
                        }
                    }
                };
                log.lock().unwrap().push(Recorded { head, body });
                let reply = responses.next().unwrap_or("{\"error\":\"unexpected\"}");
                let status = if reply.contains("\"error\"") { "400 Bad Request" } else { "200 OK" };
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                );
                let _ = stream.write_all(resp.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });
        MockTokenEndpoint { url, requests }
    }

    pub fn recorded(&self) -> Vec<Recorded> {
        self.requests.lock().unwrap().clone()
    }
}

/// A client secret whose token URI is the mock endpoint.
pub fn client_secret(token_url: &str) -> String {
    format!(
        r#"{{"installed":{{"client_id":"cid.apps.googleusercontent.com","client_secret":"sec","redirect_uris":["http://localhost"],"auth_uri":"https://accounts.google.com/o/oauth2/auth","token_uri":"{token_url}"}}}}"#
    )
}

/// Output the flow writes, shared with the test.
#[derive(Clone, Default)]
pub struct SharedOut(pub Arc<Mutex<Vec<u8>>>);

impl std::io::Write for SharedOut {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl SharedOut {
    pub fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

/// A loopback port nothing listens on right now.
pub fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

/// Sends one raw GET to the callback server and returns the full response.
pub async fn get(port: u16, target: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    let req = format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n");
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut resp = String::new();
    stream.read_to_string(&mut resp).await.unwrap();
    resp
}

/// The query value for `key` in a URL, percent-decoded.
pub fn query_value(url: &str, key: &str) -> String {
    let query = url.split_once('?').unwrap().1;
    let raw = query
        .split('&')
        .find_map(|p| p.strip_prefix(&format!("{key}=")))
        .unwrap();
    let mut out = Vec::new();
    let b = raw.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            out.push(u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap(), 16).unwrap());
            i += 3;
        } else {
            out.push(if b[i] == b'+' { b' ' } else { b[i] });
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}
