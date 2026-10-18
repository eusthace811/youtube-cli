#![forbid(unsafe_code)]
//! The `yutu mcp` server: every registry tool and the i18n resources over
//! MCP, with go-sdk v1.8.0's wire behavior.

mod log;
pub mod resources;
mod schema;
pub mod server;
pub mod tools;

pub use server::{Incoming, Message, ParseError, Peer, RpcError, Server, Session, serve_stdio};

use std::sync::Arc;
use yutu_core::ops::ServiceProvider;

/// The version reported in `serverInfo`, set at build time like Go's ldflags.
pub const VERSION: &str = match option_env!("YUTU_VERSION") {
    Some(v) => v,
    None => "",
};

/// Runs `yutu mcp --mode <mode>` and returns the process exit code.
///
/// `enter_stdio` runs before the stdio transport takes over stdin and
/// stdout, so the caller can switch auth prompts to stderr (divergence E2).
pub async fn run(
    mode: &str,
    provider: Arc<dyn ServiceProvider>,
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
        "http" => {
            log::error(
                "starting server failed",
                &[
                    ("error", "http mode is not available in this build"),
                    ("mode", mode),
                ],
            );
            1
        }
        _ => {
            log::error(
                "invalid mode",
                &[("mode", mode), ("valid_modes", "stdio, http")],
            );
            1
        }
    }
}
