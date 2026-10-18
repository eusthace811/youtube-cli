//! A stand-in `yutu mcp` for running the parity corpus before the CLI
//! integration: `mcpdriver mcp [--mode M]` serves with the YouTube client
//! pointed at `YUTU_PARITY_ENDPOINT` and no credentials.

use std::sync::{Arc, OnceLock};
use yutu_api::Service;
use yutu_api::client::BoxFuture;
use yutu_core::ops::ServiceProvider;

struct Parity(OnceLock<Service>);

impl ServiceProvider for Parity {
    fn service(&self) -> BoxFuture<'_, Result<Service, String>> {
        let svc = self.0.get_or_init(|| Service::new(None)).clone();
        Box::pin(async move { Ok(svc) })
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("mcp") {
        eprintln!("usage: mcpdriver mcp [--mode stdio|http]");
        std::process::exit(2);
    }
    let mode = match args.iter().position(|a| a == "--mode" || a == "-m") {
        Some(i) => args.get(i + 1).cloned().unwrap_or_default(),
        None => "stdio".to_string(),
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let provider = Arc::new(Parity(OnceLock::new()));
    let code = rt.block_on(yutu_mcp::run(&mode, provider, || {}));
    std::process::exit(code);
}
