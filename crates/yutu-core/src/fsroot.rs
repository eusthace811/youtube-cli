//! The file sandbox of Go's pkg/init.go. Every file a user names is opened
//! inside the root (`YUTU_ROOT`, else the working directory), so `..` and
//! symlinks cannot leave it, and secret-like extensions are refused both for
//! media reads and, unlike Go, for caption writes (divergence E1).

use cap_std::fs::{OpenOptions, OpenOptionsExt};
use std::io;
use std::sync::Arc;
use yutu_auth::root::go_path_error;
use yutu_auth::{RootDir, gopath};

pub const BLOCKED_EXTENSIONS: [&str; 8] = [
    ".json", ".env", ".key", ".pem", ".crt", ".cert", ".pfx", ".p12",
];

/// Go's `pkg.ErrBlockedFile`.
pub const BLOCKED_FILE: &str = "access denied: blocked file extension";

#[derive(Clone)]
pub struct Root {
    dir: Arc<RootDir>,
}

impl Root {
    /// The process-wide root, shared with the credential and token reads.
    pub fn global() -> Result<Root, String> {
        Ok(Root {
            dir: RootDir::global()?,
        })
    }

    pub fn open(path: &str) -> io::Result<Root> {
        Ok(Root {
            dir: Arc::new(RootDir::open(path)?),
        })
    }

    pub fn path(&self) -> &str {
        self.dir.path()
    }

    /// The unblocked handle `yutu_auth::authorize` reads secrets through.
    pub fn secrets(&self) -> Arc<RootDir> {
        self.dir.clone()
    }

    /// `pkg.OpenFile`: a media file to upload.
    pub fn open_read(&self, name: &str) -> io::Result<std::fs::File> {
        refuse_blocked(name)?;
        self.dir
            .dir()
            .open(name)
            .map(cap_std::fs::File::into_std)
            .map_err(|e| go_path_error("openat", name, e))
    }

    /// `Root.OpenFile(name, O_WRONLY|O_CREATE|O_TRUNC, 0600)`, without
    /// creating parent directories, and refusing blocked extensions.
    pub fn create_write(&self, name: &str) -> io::Result<std::fs::File> {
        refuse_blocked(name)?;
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true).mode(0o600);
        self.dir
            .dir()
            .open_with(name, &options)
            .map(cap_std::fs::File::into_std)
            .map_err(|e| go_path_error("openat", name, e))
    }
}

pub fn is_blocked(name: &str) -> bool {
    let ext = gopath::ext(name).to_lowercase();
    BLOCKED_EXTENSIONS.contains(&ext.as_str())
}

fn refuse_blocked(name: &str) -> io::Result<()> {
    if is_blocked(name) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            BLOCKED_FILE,
        ));
    }
    Ok(())
}
