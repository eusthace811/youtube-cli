#![forbid(unsafe_code)]
//! `youtube agent`: the Miffy agent over the youtube tools, a hand-written ADK
//! loop with Gemini and OpenAI clients, and ADK's console launcher.

pub mod agent;
pub mod console;
mod gojson;
pub mod launch;
pub mod launcher;
pub mod llm;
pub mod model;
pub mod skills;
pub mod tools;

use std::io::{IsTerminal, Write};
use std::sync::Arc;
use youtube_auth::Secret;
use youtube_auth::log;
use youtube_core::ops::ServiceProvider;

pub use agent::{Agent, Frontend, Verdict};
pub use launcher::{Launch, Streaming};
pub use tools::{Approval, Toolbox};

/// The `youtube agent` flags.
#[derive(Debug, Clone)]
pub struct Options {
    pub args: String,
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
    pub instruction: String,
}

/// The model settings after flags and environment, divergence E10 aside:
/// Go's SDKs already read these variables.
pub struct ModelConfig {
    pub llm: llm::Llm,
    /// Gemini's built-in search rides along only on Google's own endpoint
    /// named by no flag, as Go's agent decides it.
    pub google_search: bool,
}

/// Picks the provider client. For `claude-code` and `codex` the model is
/// the CLI's own default unless `--model` names one, and `YOUTUBE_CLI_CLAUDE_BIN`
/// or `YOUTUBE_CLI_CODEX_BIN` may name the executable. Keys come from `--api-key`, else
/// `GOOGLE_API_KEY` then `GEMINI_API_KEY` (google) or `OPENAI_API_KEY`
/// (openai); base URLs from `--base-url`, else `GOOGLE_GEMINI_BASE_URL` or
/// `OPENAI_BASE_URL`.
pub fn model_config(
    opts: &Options,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<ModelConfig, String> {
    let env = |k: &str| env(k).filter(|v| !v.is_empty());
    match opts.provider.as_str() {
        "google" => {
            let key = if opts.api_key.is_empty() {
                let (google, gemini) = (env("GOOGLE_API_KEY"), env("GEMINI_API_KEY"));
                if google.is_some() && gemini.is_some() {
                    log::warn(
                        "Both GOOGLE_API_KEY and GEMINI_API_KEY are set. Using GOOGLE_API_KEY.",
                        &[],
                    );
                }
                google.or(gemini).unwrap_or_default()
            } else {
                opts.api_key.clone()
            };
            if key.is_empty() {
                return Err("api key is required for Google AI backend.\nYou can get the API key from https://ai.google.dev/gemini-api/docs/api-key".into());
            }
            let base = if opts.base_url.is_empty() {
                env("GOOGLE_GEMINI_BASE_URL")
                    .unwrap_or_else(|| llm::gemini::DEFAULT_BASE_URL.into())
            } else {
                opts.base_url.clone()
            };
            Ok(ModelConfig {
                llm: llm::Llm::Gemini(llm::gemini::Gemini::new(
                    &opts.model,
                    Secret::new(key),
                    &base,
                )),
                google_search: opts.base_url.is_empty(),
            })
        }
        "openai" => {
            if opts.model.is_empty() {
                return Err("openai: model name is required".into());
            }
            let key = if opts.api_key.is_empty() {
                env("OPENAI_API_KEY").unwrap_or_default()
            } else {
                opts.api_key.clone()
            };
            let base = if opts.base_url.is_empty() {
                env("OPENAI_BASE_URL").unwrap_or_else(|| llm::openai::DEFAULT_BASE_URL.into())
            } else {
                opts.base_url.clone()
            };
            Ok(ModelConfig {
                llm: llm::Llm::OpenAi(llm::openai::OpenAi::new(
                    &opts.model,
                    Secret::new(key),
                    &base,
                )),
                google_search: false,
            })
        }
        "claude-code" | "codex" => {
            let kind = if opts.provider == "codex" {
                llm::cli::Kind::Codex
            } else {
                llm::cli::Kind::ClaudeCode
            };
            // The CLI signs in on its own; a key or endpoint here would be
            // silently ignored, so it is refused.
            if !opts.api_key.is_empty() || !opts.base_url.is_empty() {
                return Err(format!(
                    "{}: --api-key and --base-url do not apply; the CLI uses its own sign-in",
                    kind.provider()
                ));
            }
            let (var, default) = kind.program_var();
            let program = env(var).unwrap_or_else(|| default.to_string());
            Ok(ModelConfig {
                llm: llm::Llm::Cli(llm::cli::Cli::new(kind, program.into(), &opts.model)?),
                google_search: false,
            })
        }
        other => Err(format!(
            "unsupported provider {}: supported providers are google, openai, claude-code and codex",
            log::quote(other)
        )),
    }
}

/// Builds Miffy: model, in-process MCP tools, skills and instruction.
pub async fn build(
    opts: &Options,
    provider: Arc<dyn ServiceProvider>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Agent, (&'static str, String)> {
    let cfg = model_config(opts, env).map_err(|e| ("model configuration error", e))?;
    let tools = Toolbox::new(provider, cfg.google_search)
        .await
        .map_err(|e| ("failed to create MCP tool set", e))?;
    Ok(Agent::new(cfg.llm, tools, &opts.instruction))
}

/// Runs `youtube agent` and returns the exit code, with Go's order of checks:
/// the launcher syntax for empty `--args`, then the model, then the
/// launcher arguments.
pub fn run(opts: &Options, provider: Arc<dyn ServiceProvider>) -> i32 {
    let mut stdout = std::io::stdout();
    if opts.args.is_empty() {
        let _ = stdout.write_all(launcher::SYNTAX.as_bytes());
        return 0;
    }
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            log::error("failed to launch agent", &[("error", &e)]);
            return 1;
        }
    };
    runtime.block_on(async {
        let agent = match build(opts, provider, &|k| std::env::var(k).ok()).await {
            Ok(a) => a,
            Err((msg, e)) => {
                log::error(msg, &[("error", &e)]);
                return 1;
            }
        };
        let launch = launcher::parse(&launcher::fields(&opts.args), &mut std::io::stderr());
        let config = match launch {
            Ok(Launch::Console(c)) => c,
            Ok(Launch::Web(web)) => {
                let instruction = if opts.instruction.is_empty() {
                    agent::INSTRUCTION
                } else {
                    &opts.instruction
                };
                let google_search = opts.provider == "google" && opts.base_url.is_empty();
                let app = launch::web::App::new(
                    Arc::new(agent),
                    *web,
                    launch::wire::token(),
                    launch::wire::token(),
                    instruction,
                    google_search,
                );
                return match launch::web::run(Arc::new(app)).await {
                    Ok(()) => 0,
                    Err(e) => launch_failed(&mut stdout, &e),
                };
            }
            Err(e) => return launch_failed(&mut stdout, &e),
        };
        if config.otel_to_cloud {
            // Divergence E12.
            log::warn("telemetry is not supported in youtube-cli", &[]);
        }
        let streaming = config.streaming.unwrap_or(if stdout.is_terminal() {
            Streaming::Sse
        } else {
            Streaming::None
        });
        let interactive = youtube_core::confirm::interactive(std::io::stdin().is_terminal());
        let escape = stdout.is_terminal();
        tokio::select! {
            () = console::run(&agent, console::stdin_lines(), &mut stdout, streaming, interactive, escape) => {}
            _ = tokio::signal::ctrl_c() => {}
        }
        0
    })
}

/// Go logs the launcher error, prints the syntax to stdout and exits 1.
fn launch_failed(stdout: &mut dyn Write, error: &str) -> i32 {
    log::error("failed to launch agent", &[("error", &error)]);
    let _ = stdout.write_all(launcher::SYNTAX.as_bytes());
    1
}
