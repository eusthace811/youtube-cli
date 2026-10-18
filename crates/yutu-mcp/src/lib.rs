#![forbid(unsafe_code)]
//! The `yutu mcp` server: every registry tool and the i18n resources over
//! MCP, with go-sdk v1.8.0's wire behavior.

pub mod http;
mod log;
pub mod resources;
mod schema;
pub mod server;
pub mod tools;
pub mod verify;

pub use server::{Incoming, Message, ParseError, Peer, RpcError, Server, Session, serve_stdio};

use std::sync::Arc;
use yutu_core::ops::ServiceProvider;

/// The version reported in `serverInfo`, set at build time like Go's ldflags.
pub const VERSION: &str = match option_env!("YUTU_VERSION") {
    Some(v) => v,
    None => "",
};

/// The hidden HTTP flags of `yutu mcp`.
pub struct HttpFlags {
    pub host: String,
    pub port: i64,
    pub base_url: String,
    pub stateless: bool,
}

/// Runs `yutu mcp --mode <mode>` and returns the process exit code.
///
/// `provider` serves stdio only: in HTTP mode each request carries its own
/// verified token. `enter_stdio` runs before the stdio transport takes over
/// stdin and stdout, so the caller can switch auth prompts to stderr
/// (divergence E2).
pub async fn run(
    mode: &str,
    provider: Arc<dyn ServiceProvider>,
    http: HttpFlags,
    enter_stdio: impl FnOnce(),
) -> i32 {
    log::info(
        "starting MCP server",
        &[("mode", mode), ("version", VERSION)],
    );
    match mode {
        "stdio" => {
            enter_stdio();
            match serve_stdio(Server::new(provider, VERSION)).await {
                Ok(()) => 0,
                Err(e) => {
                    log::error("starting server failed", &[("error", &e), ("mode", mode)]);
                    1
                }
            }
        }
        "http" => match serve_http(http).await {
            Ok(()) => 0,
            Err(e) => {
                log::error("starting server failed", &[("error", &e), ("mode", mode)]);
                1
            }
        },
        _ => {
            log::error(
                "invalid mode",
                &[("mode", mode), ("valid_modes", "stdio, http")],
            );
            1
        }
    }
}

async fn serve_http(flags: HttpFlags) -> Result<(), String> {
    let addr = format!("{}:{}", flags.host, flags.port);
    log::info(
        "http server configuration",
        &[("url", &format!("http://{addr}/mcp")), ("auth", "true")],
    );
    if !flags.stateless {
        return Err("stateful HTTP sessions are not supported; run with --stateless".into());
    }
    let port = u16::try_from(flags.port)
        .map_err(|_| format!("listen tcp: address {addr}: invalid port"))?;
    let cfg = http::Config {
        host: flags.host,
        port,
        base_url: flags.base_url,
        tokeninfo_url: verify::tokeninfo_url(),
        allowed_clients: verify::allowed_clients_from_env(),
        api_base: yutu_api::client::endpoint(),
        version: VERSION.to_string(),
    };
    let listen = |e: std::io::Error| format!("listen tcp {addr}: {e}");
    http::HttpServer::bind(cfg)
        .await
        .map_err(listen)?
        .run()
        .await
        .map_err(|e| e.to_string())
}
