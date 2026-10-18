//! Turns the credential and token arguments into a refreshing token source,
//! with the decisions of Go's `refreshClient`.

use crate::flow::{self, ClientConfig, Prompt};
use crate::input;
use crate::log;
use crate::root::RootDir;
use crate::store::TokenStore;
use crate::token::Token;
use std::sync::Arc;
use std::time::Duration;
use yutu_api::client::{BoxFuture, TokenSource};

pub struct Options {
    /// Replaces the client secret's redirect URL when non-empty.
    pub redirect_url: String,
    pub timeout: Duration,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            redirect_url: flow::DEFAULT_REDIRECT_URL.to_string(),
            timeout: flow::CALLBACK_TIMEOUT,
        }
    }
}

/// Bearer tokens for the YouTube client. It refreshes an expired token and,
/// unlike Go, saves every refresh to the token file (divergence E3).
pub struct FileTokenSource {
    config: ClientConfig,
    store: Option<TokenStore>,
    token: tokio::sync::Mutex<Token>,
}

impl FileTokenSource {
    pub async fn current(&self) -> Result<Token, String> {
        let mut token = self.token.lock().await;
        if !token.valid() {
            let fresh = flow::refresh(&self.config, &token.refresh_token).await?;
            if let Some(store) = &self.store {
                // A failed save is logged by the store; the fresh token still
                // serves this process.
                let _ = store.save(&fresh);
            }
            *token = fresh;
        }
        Ok(token.clone())
    }
}

impl TokenSource for FileTokenSource {
    fn token(&self) -> BoxFuture<'_, Result<String, String>> {
        Box::pin(async move { Ok(self.current().await?.access_token) })
    }
}

/// `NewY2BService(WithCredential, WithCacheToken, WithRedirectURL)` then
/// `refreshClient`: logs in through the browser when there is no usable
/// token, refreshes an expired one, and saves what it obtained.
pub async fn authorize(
    root: Arc<RootDir>,
    credential: &str,
    cache_token: &str,
    options: Options,
    prompt: Prompt,
) -> Result<FileTokenSource, String> {
    let secret = input::resolve_credential(&root, credential);
    let cache = input::resolve_cache_token(&root, cache_token);
    let secret = secret?;
    let mut config = ClientConfig::from_json(secret.expose())
        .map_err(|e| format!("failed to parse client secret: {e}"))?;
    if !options.redirect_url.is_empty() {
        config.redirect_url = options.redirect_url;
    }
    let store = cache.file.map(|file| TokenStore::new(root.clone(), file));
    let save = |token: &Token| match &store {
        Some(store) => store.save(token),
        None => Ok(()),
    };

    let token = match Token::from_go_json(cache.json.expose()) {
        Err(_) => {
            let token = flow::token_from_web(&config, options.timeout, prompt).await?;
            save(&token)?;
            token
        }
        Ok(token) if token.valid() => token,
        Ok(expired) => match flow::refresh(&config, &expired.refresh_token).await {
            Ok(fresh) => {
                save(&fresh)?;
                fresh
            }
            Err(_) if store.is_some() => {
                let token = flow::token_from_web(&config, options.timeout, prompt).await?;
                save(&token)?;
                token
            }
            Err(e) => {
                return Err(format!(
                    "failed to refresh token, please re-authenticate in cli: {e}"
                ));
            }
        },
    };
    Ok(FileTokenSource {
        config,
        store,
        token: tokio::sync::Mutex::new(token),
    })
}

/// The file-based branch of Go's `defaultYouTubeService`: env or default
/// arguments under the process root, prompting on the terminal.
pub async fn default_token_source() -> Result<Arc<FileTokenSource>, String> {
    let wrap = |e: String| format!("failed to create YouTube service: {e}");
    let root = RootDir::global().map_err(wrap)?;
    authorize(root, "", "", Options::default(), Prompt::terminal())
        .await
        .map(Arc::new)
        .map_err(wrap)
}

/// `yutu auth`: on failure logs `authentication failed` and returns exit
/// code 1. Builds its own runtime, so call it outside any async context.
pub fn run_auth_command(credential: &str, cache_token: &str) -> Result<(), i32> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| {
            log::error("authentication failed", &[("error", &e)]);
            1
        })?;
    let result = runtime.block_on(async {
        let root = RootDir::global()?;
        authorize(
            root,
            credential,
            cache_token,
            Options::default(),
            Prompt::terminal(),
        )
        .await
    });
    result.map(|_| ()).map_err(|e| {
        log::error("authentication failed", &[("error", &e)]);
        1
    })
}
