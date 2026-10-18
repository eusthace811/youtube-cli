//! A gated tool called by a client that answers the server's
//! `elicitation/create`: declining must keep the op from reaching the API.

use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use yutu_api::Service;
use yutu_api::client::BoxFuture;
use yutu_core::ops::ServiceProvider;
use yutu_mcp::{Incoming, Peer, Server, Session};

/// An HTTP endpoint that counts requests and answers each with 204.
async fn recorder() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    tokio::spawn(async move {
        while let Ok((mut conn, _)) = listener.accept().await {
            let counter = counter.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                while let Ok(n) = conn.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    counter.fetch_add(1, Ordering::SeqCst);
                    let _ = conn
                        .write_all(b"HTTP/1.1 204 No Content\r\ncontent-length: 0\r\n\r\n")
                        .await;
                }
            });
        }
    });
    (url, hits)
}

struct At(String);

impl ServiceProvider for At {
    fn service(&self) -> BoxFuture<'_, Result<Service, String>> {
        let svc = Service::with_base(self.0.clone(), None);
        Box::pin(async move { Ok(svc) })
    }
}

fn message(frame: Value) -> yutu_mcp::Message {
    match Incoming::parse(&frame.to_string()).unwrap() {
        Incoming::Single(m) => m,
        Incoming::Batch(_) => panic!("batch"),
    }
}

/// Calls `video-delete` as a 2025-06-18 client that answers the server's
/// elicitation with `action`, and returns the call's result.
async fn delete_answering(action: &str, url: String) -> Value {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let session = Session::new(Server::new(Arc::new(At(url)), ""), Some(Peer::new(tx)));
    session
        .handle(message(
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {"elicitation": {}},
            "clientInfo": {"name": "elicit", "version": "0"}}}),
        ))
        .await
        .unwrap();
    let call = {
        let session = session.clone();
        tokio::spawn(async move {
            session
                .handle(message(
                    json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                    "params": {"name": "video-delete", "arguments": {"ids": ["v1"]}}}),
                ))
                .await
                .unwrap()
        })
    };
    let request: Value = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
    assert_eq!(request["method"], "elicitation/create");
    assert_eq!(
        request["params"],
        json!({"mode": "form", "message": "Delete video(s): v1"})
    );
    session
        .handle(message(
            json!({"jsonrpc": "2.0", "id": request["id"], "result": {"action": action}}),
        ))
        .await;
    let reply: Value = serde_json::from_str(&call.await.unwrap()).unwrap();
    reply["result"].clone()
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(f)
}

#[test]
fn declining_sends_nothing_to_the_api() {
    block_on(async {
        let (url, hits) = recorder().await;
        let result = delete_answering("decline", url).await;
        assert_eq!(
            result,
            json!({"content": [{"type": "text", "text": "canceled by user"}], "isError": true})
        );
        assert_eq!(hits.load(Ordering::SeqCst), 0);
    })
}

#[test]
fn accepting_reaches_the_api() {
    block_on(async {
        let (url, hits) = recorder().await;
        let result = delete_answering("accept", url).await;
        assert_eq!(
            result,
            json!({"content": [{"type": "text", "text": "Video v1 deleted"}]})
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    })
}
