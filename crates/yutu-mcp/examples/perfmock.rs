//! The upstream for the R8 throughput probe: tokeninfo and `videos.list`
//! answered from memory on all cores. parity/recorder.py serves about
//! 2,000 requests a second, so behind it the probe would time the recorder.
//!
//!     cargo run --release -p yutu-mcp --example perfmock -- 18300 parity/fixtures/GET_videos.json

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use std::convert::Infallible;
use std::sync::Arc;
use tokio::net::TcpListener;

const SCOPES: &str = "https://www.googleapis.com/auth/youtube https://www.googleapis.com/auth/youtube.force-ssl https://www.googleapis.com/auth/youtube.channel-memberships.creator";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [port, fixture] = &args[..] else {
        eprintln!("usage: perfmock <port> <GET_videos.json>");
        std::process::exit(2);
    };
    let fixture: Value =
        serde_json::from_slice(&std::fs::read(fixture).expect("fixture")).expect("JSON");
    let mut page = fixture["pages"][0].clone();
    if let Value::Object(m) = &mut page {
        m.remove("_token");
    }
    let videos = Bytes::from(page.to_string());
    let info = Bytes::from(
        json!({"aud": "client-a", "azp": "client-a", "sub": "1001", "scope": SCOPES, "expires_in": "3599"})
            .to_string(),
    );
    let bodies = Arc::new((videos, info));
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime");
    rt.block_on(async move {
        let listener = TcpListener::bind(("127.0.0.1", port.parse::<u16>().expect("port")))
            .await
            .expect("bind");
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let bodies = bodies.clone();
            tokio::spawn(async move {
                let svc = hyper::service::service_fn(move |req: Request<hyper::body::Incoming>| {
                    let bodies = bodies.clone();
                    async move {
                        let path = req.uri().path().to_string();
                        let _ = req.into_body().collect().await;
                        let body = match path.as_str() {
                            "/tokeninfo" => bodies.1.clone(),
                            _ => bodies.0.clone(),
                        };
                        let mut resp = Response::new(Full::new(body));
                        resp.headers_mut()
                            .insert("content-type", "application/json".parse().unwrap());
                        Ok::<_, Infallible>(resp)
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), svc)
                    .await;
            });
        }
    });
}
