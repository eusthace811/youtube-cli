//! The R8 throughput probe's client: N keep-alive HTTP/1.1 connections
//! posting one MCP request for a fixed time. `ab` speaks HTTP/1.0, which
//! makes Go close the connection after every chunked reply.
//!
//!     loadgen <url> <seconds> <clients> <body.json>
//!
//! Prints one JSON line: requests/second, p50 and p99 in ms, and how many
//! replies were not HTTP 200 or carried a tool error.

use std::time::{Duration, Instant};

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [url, seconds, clients, body] = &args[..] else {
        eprintln!("usage: loadgen <url> <seconds> <clients> <body.json>");
        std::process::exit(2);
    };
    let body = std::fs::read(body).expect("body");
    let clients: usize = clients.parse().expect("clients");
    let run = Duration::from_secs_f64(seconds.parse().expect("seconds"));
    let _ = rustls::crypto::ring::default_provider().install_default();
    let http = reqwest::Client::builder()
        .pool_max_idle_per_host(clients)
        .http1_only()
        .build()
        .expect("client");
    let deadline = Instant::now() + run;
    let start = Instant::now();
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..clients {
        let (http, url, body) = (http.clone(), url.clone(), body.clone());
        tasks.spawn(async move {
            let (mut lat, mut failed) = (Vec::new(), 0u64);
            while Instant::now() < deadline {
                let t = Instant::now();
                let resp = http
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .header("Accept", "application/json, text/event-stream")
                    .header("Authorization", "Bearer good-token")
                    .header("Mcp-Protocol-Version", "2025-06-18")
                    .body(body.clone())
                    .send()
                    .await;
                let ok = match resp {
                    Ok(r) if r.status() == 200 => r
                        .bytes()
                        .await
                        .is_ok_and(|b| !b.windows(15).any(|w| w == b"\"isError\":true")),
                    _ => false,
                };
                lat.push(t.elapsed());
                failed += u64::from(!ok);
            }
            (lat, failed)
        });
    }
    let (mut lat, mut failed) = (Vec::new(), 0);
    while let Some(Ok((l, f))) = tasks.join_next().await {
        lat.extend(l);
        failed += f;
    }
    let elapsed = start.elapsed().as_secs_f64();
    lat.sort();
    let pct =
        |p: f64| lat[((lat.len() as f64 * p) as usize).min(lat.len() - 1)].as_secs_f64() * 1e3;
    println!(
        "{{\"done\":{},\"rps\":{:.1},\"p50_ms\":{:.3},\"p99_ms\":{:.3},\"failed\":{failed}}}",
        lat.len(),
        lat.len() as f64 / elapsed,
        pct(0.5),
        pct(0.99)
    );
}
