#![forbid(unsafe_code)]

mod version;

use std::io::Write;
use std::process::ExitCode;

use yutu_core::cli::{self, App, CmdError, Invocation};

struct Yutu;

impl App for Yutu {
    fn pre_run(&self, _inv: &Invocation) -> Result<(), CmdError> {
        Ok(())
    }

    fn run(&self, inv: &Invocation) -> Result<(), CmdError> {
        match inv.command.path {
            "version" => {
                // Go prints the banner with cobra's cmd.Printf, which writes to stderr.
                let _ =
                    std::io::stderr().write_all(version::banner(&version::process_env).as_bytes());
                Ok(())
            }
            _ => Err(CmdError::Message("not implemented".to_string())),
        }
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
