//! A scripted Gemini endpoint and a YouTube recorder for console sessions.

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

/// Serves HTTP/1.1 on loopback; `answer` maps (request line, body) to a
/// JSON reply, and every request is logged.
async fn serve(answer: impl Fn(&str, &str) -> Value + Send + Sync + 'static) -> (String, Log) {
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
                    let reply = answer(&line, &body).to_string();
                    seen.lock().unwrap().push((line, body));
                    let head = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n",
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

/// A Gemini `generateContent` endpoint replaying `script`: each step is
/// `{"text": ...}` or `{"calls": [{"name", "args"}]}`.
pub async fn mock_llm(script: Vec<Value>) -> (String, Log) {
    let step = Arc::new(Mutex::new(0usize));
    serve(move |_, _| {
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
        json!({"candidates": [{"content": {"role": "model", "parts": parts}, "finishReason": "STOP"}]})
    })
    .await
}

/// A YouTube API stand-in answering every call with one item.
pub async fn recorder() -> (String, Log) {
    serve(|_, _| json!({"items": [{"id": "item-1", "snippet": {"title": "Item one"}}]})).await
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
