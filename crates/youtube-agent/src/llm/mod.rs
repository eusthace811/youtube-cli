//! The model clients: Gemini's `generateContent` API, as genai sends it for
//! ADK, OpenAI's Responses API, as ADK's openai adapter sends it, and the
//! Claude Code and Codex CLIs (divergence E16).

pub mod cli;
pub mod gemini;
pub mod openai;
mod sse;

use std::time::Duration;

use serde_json::Value;

use crate::model::{Content, Request};

/// What ADK keeps from a model response beside its content; the web
/// launchers put it on each event.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Meta {
    /// genai's `usageMetadata` object.
    pub usage: Option<Value>,
    pub finish_reason: Option<String>,
    pub model_version: Option<String>,
    pub turn_complete: bool,
}

/// A whole model reply.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub content: Content,
    pub meta: Meta,
}

/// Called with each chunk of a streamed reply as it arrives.
pub type OnChunk<'a> = &'a mut (dyn FnMut(&Content, &Meta) + Send);

pub enum Llm {
    Gemini(gemini::Gemini),
    OpenAi(openai::OpenAi),
    Cli(cli::Cli),
}

impl Llm {
    pub async fn generate(
        &self,
        req: &Request<'_>,
        stream: bool,
        on_chunk: OnChunk<'_>,
    ) -> Result<Reply, String> {
        match self {
            Llm::Gemini(g) => g.generate(req, stream, on_chunk).await,
            Llm::OpenAi(o) => o.generate(req, stream, on_chunk).await,
            Llm::Cli(c) => c.generate(req, stream, on_chunk).await,
        }
    }
}

/// The HTTP client both providers share. Generation can think for minutes,
/// so the idle read bound is generous; `connect` stays short. Redirects are
/// not followed: the API key travels in a custom header that reqwest would
/// carry to whatever host a redirect names.
pub(crate) fn http_client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(300))
        .user_agent(concat!("youtube-cli/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("TLS backend available")
}

/// The whole-request bound for a non-streamed call.
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

/// A transport error with its causes, so "connection refused" reaches the
/// console as it does in Go.
pub(crate) fn describe(e: &reqwest::Error) -> String {
    let mut msg = e.to_string();
    let mut cause = std::error::Error::source(e);
    while let Some(c) = cause {
        let text = c.to_string();
        if !msg.contains(&text) {
            msg = format!("{msg}: {text}");
        }
        cause = c.source();
    }
    msg
}

pub(crate) fn join_url(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}
