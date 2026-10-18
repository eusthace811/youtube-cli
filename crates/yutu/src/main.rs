#![forbid(unsafe_code)]

mod version;

use std::io::{IsTerminal, Write};
use std::process::ExitCode;

use yutu_api::Service;
use yutu_api::client::BoxFuture;
use yutu_core::cli::{self, App, CmdError, Invocation};
use yutu_core::ops::{self, Ctx, ServiceProvider};
use yutu_core::output::Out;

struct Yutu;

/// The service for CLI and stdio MCP commands, as Go's file-based
/// `defaultYouTubeService`: credentials and token from flags, env or defaults
/// under the root. Builds with `parity-endpoint` that point at the recorder
/// skip OAuth unless `YUTU_PARITY_AUTH=1` asks the lanes to exercise it.
struct CliService;

impl ServiceProvider for CliService {
    fn service(&self) -> BoxFuture<'_, Result<Service, String>> {
        Box::pin(async {
            if yutu_api::client::parity_mode()
                && std::env::var("YUTU_PARITY_AUTH").as_deref() != Ok("1")
            {
                return Ok(Service::new(None));
            }
            let tokens = yutu_auth::default_token_source().await?;
            Ok(Service::new(Some(tokens)))
        })
    }
}

impl App for Yutu {
    fn pre_run(&self, inv: &Invocation) -> Result<(), CmdError> {
        let Some(entry) = ops::find(inv.command.path) else {
            return Ok(());
        };
        let op = (entry.cli)(&inv.flags);
        let message = op.confirm();
        if message.is_empty() {
            return Ok(());
        }
        yutu_core::confirm::confirm(inv.flags.bool("yes"), &message).map_err(CmdError::Message)
    }

    fn run(&self, inv: &Invocation) -> Result<(), CmdError> {
        if inv.command.path == "auth" {
            return yutu_auth::run_auth_command(
                inv.flags.string("credential"),
                inv.flags.string("cacheToken"),
            )
            .map_err(CmdError::Exit);
        }
        if inv.command.path == "version" {
            // Go prints the banner with cobra's cmd.Printf, which writes to stderr.
            let _ = std::io::stderr().write_all(version::banner(&version::process_env).as_bytes());
            return Ok(());
        }
        let Some(entry) = ops::find(inv.command.path) else {
            return Err(CmdError::Message("not implemented".to_string()));
        };
        let op = (entry.cli)(&inv.flags);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| CmdError::Message(e.to_string()))?;
        let mut stdout = std::io::stdout();
        let interactive = yutu_core::confirm::interactive(stdout.is_terminal());
        let result = {
            let mut ctx = Ctx::new(
                Out {
                    w: &mut stdout,
                    interactive,
                },
                &CliService,
            );
            runtime.block_on(op.run(&mut ctx))
        };
        let _ = stdout.flush();
        result.map_err(|e| CmdError::Message(e.to_string()))
    }
}

fn main() -> ExitCode {
    let args = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let code = cli::execute(args, &Yutu);
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
