//! The directory every file access is confined to, as Go's `os.Root`.
//! This handle reads secrets without the media extension blocklist, as Go
//! reads them through `Root.FS()`; `youtube_core::fsroot` adds the blocklist.

use crate::{gopath, log};
use cap_std::fs::{Dir, DirBuilder, DirBuilderExt, OpenOptions, OpenOptionsExt};
use std::fmt;
use std::io::{self, Write};
use std::sync::{Arc, OnceLock};

pub struct RootDir {
    path: String,
    dir: Dir,
}

impl RootDir {
    pub fn open(path: &str) -> io::Result<RootDir> {
        let dir = Dir::open_ambient_dir(path, cap_std::ambient_authority())
            .map_err(|e| go_path_error("open", path, e))?;
        Ok(RootDir {
            path: path.to_string(),
            dir,
        })
    }

    /// `YOUTUBE_CLI_ROOT`, else the working directory, else `/`, as Go's
    /// `initRootDir` picks it.
    pub fn from_env() -> io::Result<RootDir> {
        let path = match std::env::var("YOUTUBE_CLI_ROOT") {
            Ok(p) => p,
            Err(_) => gopath::getwd().unwrap_or_else(|e| {
                log::debug(
                    "failed to get working directory",
                    &[("error", &e), ("fallback", &"/")],
                );
                "/".to_string()
            }),
        };
        match RootDir::open(&path) {
            Ok(root) => {
                log::debug("Root directory set", &[("dir", &path)]);
                Ok(root)
            }
            Err(e) => {
                log::error(
                    "failed to open root directory",
                    &[("dir", &path), ("error", &e)],
                );
                Err(e)
            }
        }
    }

    /// The process-wide root, opened from the environment on first use.
    pub fn global() -> Result<Arc<RootDir>, String> {
        static ROOT: OnceLock<Result<Arc<RootDir>, String>> = OnceLock::new();
        ROOT.get_or_init(|| RootDir::from_env().map(Arc::new).map_err(|e| e.to_string()))
            .clone()
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn dir(&self) -> &Dir {
        &self.dir
    }

    /// `fs.Stat(root.FS(), name)`: `name` must be a valid `io/fs` path.
    pub fn stat(&self, name: &str) -> io::Result<cap_std::fs::Metadata> {
        if !valid_fs_path(name) {
            return Err(go_path_error(
                "stat",
                name,
                io::Error::from(io::ErrorKind::InvalidInput),
            ));
        }
        self.dir
            .metadata(name)
            .map_err(|e| go_path_error("statat", name, e))
    }

    /// `fs.ReadFile(root.FS(), name)`, with no extension blocklist.
    pub fn read_unblocked(&self, name: &str) -> io::Result<Vec<u8>> {
        if !valid_fs_path(name) {
            return Err(go_path_error(
                "open",
                name,
                io::Error::from(io::ErrorKind::InvalidInput),
            ));
        }
        self.dir
            .read(name)
            .map_err(|e| go_path_error("read", name, e))
    }

    /// Replaces `name` with `bytes` through a mode-0600 temporary file and a
    /// rename, creating parent directories with mode 0755, so a crash never
    /// leaves a truncated file and a reader never sees a partial one.
    pub fn write_atomic(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let (parent, base) = match name.rfind('/') {
            Some(i) => (&name[..i], &name[i + 1..]),
            None => ("", name),
        };
        if !parent.is_empty() {
            let mut builder = DirBuilder::new();
            builder.recursive(true).mode(0o755);
            self.dir
                .create_dir_with(parent, &builder)
                .map_err(|e| go_path_error("mkdirat", parent, e))?;
        }
        let mut suffix = [0u8; 8];
        getrandom::fill(&mut suffix).map_err(|e| io::Error::other(e.to_string()))?;
        let tmp_base: String = suffix.iter().map(|b| format!("{b:02x}")).collect();
        let tmp = if parent.is_empty() {
            format!(".{base}.{tmp_base}.tmp")
        } else {
            format!("{parent}/.{base}.{tmp_base}.tmp")
        };
        let mut options = OpenOptions::new();
        options.write(true).create_new(true).mode(0o600);
        let mut file = self
            .dir
            .open_with(&tmp, &options)
            .map_err(|e| go_path_error("openat", &tmp, e))?;
        let written = file
            .write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| go_path_error("write", &tmp, e))
            .and_then(|()| {
                self.dir
                    .rename(&tmp, &self.dir, name)
                    .map_err(|e| go_path_error("renameat", name, e))
            });
        if written.is_err() {
            let _ = self.dir.remove_file(&tmp);
        }
        written
    }
}

/// `io/fs.ValidPath`.
fn valid_fs_path(name: &str) -> bool {
    name == "."
        || name
            .split('/')
            .all(|e| !e.is_empty() && e != "." && e != "..")
}

/// An `io::Error` that prints like Go's `*fs.PathError`, e.g.
/// `openat ../x: path escapes from parent`.
#[derive(Debug)]
struct GoPathError {
    op: &'static str,
    path: String,
    text: String,
}

impl fmt::Display for GoPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.op, self.path, self.text)
    }
}

impl std::error::Error for GoPathError {}

pub fn go_path_error(op: &'static str, path: &str, err: io::Error) -> io::Error {
    if err.get_ref().is_some_and(|inner| inner.is::<GoPathError>()) {
        return err;
    }
    let text = match err.raw_os_error() {
        Some(code) => go_errno_text(code),
        // cap-std reports an escape from the root as a bare PermissionDenied.
        None if err.kind() == io::ErrorKind::PermissionDenied => {
            "path escapes from parent".to_string()
        }
        None if err.kind() == io::ErrorKind::InvalidInput => "invalid argument".to_string(),
        None => err.to_string(),
    };
    io::Error::new(
        err.kind(),
        GoPathError {
            op,
            path: path.to_string(),
            text,
        },
    )
}

/// Go's `syscall.Errno` text: the C library message, lower-cased.
pub(crate) fn go_errno_text(code: i32) -> String {
    let msg = io::Error::from_raw_os_error(code).to_string();
    let msg = match msg.rfind(" (os error ") {
        Some(i) => &msg[..i],
        None => &msg,
    };
    let mut chars = msg.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_fs_paths() {
        assert!(valid_fs_path("."));
        assert!(valid_fs_path("a/b.json"));
        assert!(!valid_fs_path(""));
        assert!(!valid_fs_path("../x"));
        assert!(!valid_fs_path("a/./b"));
        assert!(!valid_fs_path("/abs"));
    }

    #[test]
    fn errno_text_is_go_spelled() {
        assert_eq!(go_errno_text(2), "no such file or directory");
        assert_eq!(go_errno_text(20), "not a directory");
    }
}
