//! A scripted Gemini endpoint, YouTube stand-ins, and harnesses for console
//! sessions and web launchers.
#![allow(dead_code)]

use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use yutu_agent::{Agent, Options, Streaming};
use yutu_api::Service;
use yutu_api::client::BoxFuture;
use yutu_core::ops::ServiceProvider;

pub type Log = Arc<Mutex<Vec<(String, String)>>>;

/// A reply body and its content type.
pub type Reply = (&'static str, String);

/// Serves HTTP/1.1 on loopback; `answer` maps (request line, body) to a
/// reply, and every request is logged.
async fn serve(answer: impl Fn(&str, &str) -> Reply + Send + Sync + 'static) -> (String, Log) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let log: Log = Arc::default();
    let (answer, seen) = (Arc::new(answer), log.clone());
    tokio::spawn(async move {
        while let Ok((conn, _)) = listener.accept().await {
            let (answer, seen) = (answer.clone(), seen.clone());
            tokio::spawn(async move {
                let (r, mut w) = conn.into_split();
                let mut r = BufReader::new(r);
                loop {
                    let mut request_line = String::new();
                    if r.read_line(&mut request_line).await.unwrap_or(0) == 0 {
                        return;
                    }
                    let mut len = 0;
                    loop {
                        let mut h = String::new();
                        r.read_line(&mut h).await.unwrap();
                        if h.trim().is_empty() {
                            break;
                        }
                        if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                            len = v.trim().parse().unwrap();
                        }
                    }
                    let mut body = vec![0; len];
                    r.read_exact(&mut body).await.unwrap();
                    let (line, body) = (
                        request_line.trim().to_string(),
                        String::from_utf8(body).unwrap(),
                    );
                    let (ctype, reply) = answer(&line, &body);
                    seen.lock().unwrap().push((line, body));
                    let head = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\n\r\n",
                        reply.len()
                    );
                    w.write_all(head.as_bytes()).await.unwrap();
                    w.write_all(reply.as_bytes()).await.unwrap();
                }
            });
        }
    });
    (url, log)
}

/// A Gemini endpoint replaying `script`, as parity/mockllm does: each step
/// is `{"text": ...}` or `{"calls": [{"name", "args"}]}`, answered as one
/// JSON response, or one SSE chunk for `streamGenerateContent`.
pub async fn mock_llm(script: Vec<Value>) -> (String, Log) {
    let step = Arc::new(Mutex::new(0usize));
    serve(move |line, _| {
        let mut i = step.lock().unwrap();
        let s = script.get(*i).cloned().unwrap_or(json!({"text": "done"}));
        *i += 1;
        let parts: Vec<Value> = match s.get("calls") {
            Some(calls) => calls
                .as_array()
                .unwrap()
                .iter()
                .map(|c| json!({"functionCall": {"name": c["name"], "args": c["args"]}}))
                .collect(),
            None => vec![json!({"text": s["text"]})],
        };
        let reply = json!({
            "candidates": [{"content": {"role": "model", "parts": parts}, "finishReason": "STOP", "index": 0}],
            "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 1, "totalTokenCount": 2},
            "modelVersion": "mock",
        });
        if line.contains("streamGenerateContent") {
            ("text/event-stream", format!("data: {reply}\n\n"))
        } else {
            ("application/json", reply.to_string())
        }
    })
    .await
}

/// A YouTube API stand-in answering every call with one item.
pub async fn recorder() -> (String, Log) {
    serve(|_, _| {
        let reply = json!({"items": [{"id": "item-1", "snippet": {"title": "Item one"}}]});
        ("application/json", reply.to_string())
    })
    .await
}

/// The YouTube stand-in parity/recorder.py is: GETs answered with the first
/// page of parity/fixtures, writes echoed, deletes with no content.
pub async fn parity_youtube() -> (String, Log) {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../parity/fixtures");
    serve(move |line, body| {
        let mut words = line.split(' ');
        let method = words.next().unwrap_or_default();
        let path = words
            .next()
            .unwrap_or_default()
            .split('?')
            .next()
            .unwrap_or_default();
        let path = path
            .trim_start_matches('/')
            .trim_start_matches("youtube/v3/");
        let file = fixtures.join(format!("{method}_{}.json", path.replace('/', ".")));
        let fixture: Option<Value> = std::fs::read_to_string(file)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok());
        let reply = match fixture {
            Some(mut f) if f.get("pages").is_some() => {
                let mut page = f["pages"][0].take();
                page.as_object_mut().unwrap().remove("_token");
                page
            }
            Some(f) => f,
            None => {
                let mut v: Value = serde_json::from_str(body).unwrap_or(json!({}));
                if !v.is_object() {
                    v = json!({});
                }
                v.as_object_mut()
                    .unwrap()
                    .entry("id")
                    .or_insert("stub-id".into());
                v
            }
        };
        ("application/json", reply.to_string())
    })
    .await
}

struct At(String);

impl ServiceProvider for At {
    fn service(&self) -> BoxFuture<'_, Result<Service, String>> {
        let svc = Service::with_base(format!("{}/", self.0), None);
        Box::pin(async move { Ok(svc) })
    }
}

pub async fn agent(llm_url: &str, youtube_url: &str) -> Agent {
    let opts = Options {
        args: "console".into(),
        provider: "google".into(),
        model: "gemini-3.8-flash".into(),
        api_key: "llm-key".into(),
        base_url: llm_url.into(),
        instruction: String::new(),
    };
    let provider = Arc::new(At(youtube_url.to_string()));
    match yutu_agent::build(&opts, provider, &|_| None).await {
        Ok(a) => a,
        Err((msg, e)) => panic!("{msg}: {e}"),
    }
}

/// Runs a console session over `stdin` and returns what it printed.
pub async fn console(agent: &Agent, stdin: &[&str]) -> String {
    let (tx, rx) = mpsc::channel(stdin.len().max(1));
    for line in stdin {
        tx.send(format!("{line}\n")).await.unwrap();
    }
    drop(tx);
    let mut out: Vec<u8> = Vec::new();
    yutu_agent::console::run(agent, rx, &mut out, Streaming::None).await;
    String::from_utf8(out).unwrap()
}

pub fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(f)
}

/// A web launcher on a free loopback port, its agent wired to `script` and
/// the parity YouTube stand-in.
pub struct Web {
    pub port: u16,
    pub youtube: Log,
    pub model: Log,
}

pub const TOKEN: &str = "test-token-0123456789abcdef";

pub async fn web(args: &str, script: Vec<Value>) -> Web {
    let (llm, model) = mock_llm(script).await;
    let (yt, youtube) = parity_youtube().await;
    let agent = agent(&llm, &yt).await;
    let parsed =
        yutu_agent::launcher::parse(&yutu_agent::launcher::fields(args), &mut std::io::sink());
    let Ok(yutu_agent::Launch::Web(cfg)) = parsed else {
        panic!("not a web launch: {parsed:?}");
    };
    let app = yutu_agent::launch::web::App::new(
        Arc::new(agent),
        *cfg,
        TOKEN.into(),
        yutu_agent::agent::INSTRUCTION,
        false,
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(yutu_agent::launch::web::serve(
        Arc::new(app),
        listener,
        std::future::pending(),
    ));
    Web {
        port,
        youtube,
        model,
    }
}

pub struct Response {
    pub status: u16,
    /// Lowercased names, in arrival order.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// One HTTP/1.1 exchange over a fresh connection; `headers` may override
/// Host.
pub async fn http(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Response {
    let mut conn = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    let mut req = format!("{method} {path} HTTP/1.1\r\nConnection: close\r\n");
    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) {
        req.push_str(&format!("Host: 127.0.0.1:{port}\r\n"));
    }
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    if !body.is_empty() || matches!(method, "POST" | "PUT" | "PATCH") {
        req.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    req.push_str("\r\n");
    conn.write_all(req.as_bytes()).await.unwrap();
    // A server refusing an oversized body may close before reading it all.
    let _ = conn.write_all(body).await;
    let mut raw = Vec::new();
    let _ = conn.read_to_end(&mut raw).await;
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("response head");
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut rest = raw[split + 4..].to_vec();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    if headers
        .iter()
        .any(|(k, v)| k == "transfer-encoding" && v == "chunked")
    {
        let mut body = Vec::new();
        let mut i = 0;
        loop {
            let end = i + rest[i..].windows(2).position(|w| w == b"\r\n").unwrap();
            let size =
                usize::from_str_radix(std::str::from_utf8(&rest[i..end]).unwrap().trim(), 16)
                    .unwrap();
            if size == 0 {
                break;
            }
            body.extend_from_slice(&rest[end + 2..end + 2 + size]);
            i = end + 2 + size + 2;
        }
        rest = body;
    }
    Response {
        status,
        headers,
        body: rest,
    }
}
