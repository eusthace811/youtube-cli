//! Persists tokens in the bytes Go's `json.MarshalWrite` produces, so the Go
//! and Rust binaries share one token file.

use crate::log;
use crate::root::RootDir;
use crate::token::Token;
use std::sync::Arc;

pub struct TokenStore {
    root: Arc<RootDir>,
    /// Relative to the root, as Go's `tokenFile`.
    file: String,
}

impl TokenStore {
    pub fn new(root: Arc<RootDir>, file: String) -> TokenStore {
        TokenStore { root, file }
    }

    pub fn file(&self) -> &str {
        &self.file
    }

    /// Atomically replaces the file with `token` at mode 0600.
    pub fn save(&self, token: &Token) -> Result<(), String> {
        match self.root.write_atomic(&self.file, &token.to_go_json()) {
            Ok(()) => {
                log::debug("Token cached to file", &[("file", &self.file)]);
                Ok(())
            }
            Err(e) => {
                log::error(
                    "failed to cache token",
                    &[("file", &self.file), ("error", &e)],
                );
                Err(format!("failed to cache token: {e}"))
            }
        }
    }
}
