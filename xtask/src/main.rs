#![forbid(unsafe_code)]

mod egress;
mod emit;
mod gen_api;
mod import_registry;
mod registry;
mod skillgen;
mod smokegen;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("egress") => egress::run(&args[1..]),
        Some("import-registry") => import_registry::run(&args[1..]),
        Some("gen-api") => gen_api::run(&args[1..]),
        Some("skillgen") => skillgen::run(&args[1..]),
        Some("smokegen") => smokegen::run(&args[1..]),
        _ => Err(
            "usage: xtask <egress BINARY... | import-registry | gen-api [--check] \
             | skillgen [--check] | smokegen [--check]>"
                .to_string(),
        ),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

pub fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}
