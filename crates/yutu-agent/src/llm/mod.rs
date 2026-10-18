//! The two model clients: Gemini's `generateContent` API, as genai sends it
//! for ADK, and OpenAI's Responses API, as ADK's openai adapter sends it.

pub mod gemini;
pub mod openai;
mod sse;

use std::time::Duration;

use crate::model::{Content, Request};

/// Called with each text chunk of a streamed reply as it arrives.
pub type OnText<'a> = &'a mut (dyn FnMut(&str) + Send);

pub enum Llm {
    Gemini(gemini::Gemini),
    OpenAi(openai::OpenAi),
}

impl Llm {
    pub async fn generate(
        &self,
        req: &Request<'_>,
        stream: bool,
        on_text: OnText<'_>,
    ) -> Result<Content, String> {
        match self {
            Llm::Gemini(g) => g.generate(req, stream, on_text).await,
            Llm::OpenAi(o) => o.generate(req, stream, on_text).await,
        }
    }
}

/// The HTTP client both providers share. Generation can think for minutes,
/// so the idle read bound is generous; `connect` stays short.
pub(crate) fn http_client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(300))
        .user_agent(concat!("yutu-rs/", env!("CARGO_PKG_VERSION")))
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
