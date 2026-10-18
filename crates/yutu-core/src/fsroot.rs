//! Placeholder sandbox until R3's cap-std root lands: extension blocking and
//! lexical confinement to the root directory.

use std::path::{Component, Path, PathBuf};

pub fn root_dir() -> PathBuf {
    std::env::var_os("YUTU_ROOT")
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn open_media(name: &str) -> Result<yutu_api::Media, String> {
    let ext = Path::new(name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
        .unwrap_or_default();
    if crate::ops::common::BLOCKED_EXTENSIONS.contains(&ext.as_str()) {
        return Err("access denied: blocked file extension".into());
    }
    let p = Path::new(name);
    if p.is_absolute() || p.components().any(|c| c == Component::ParentDir) {
        return Err(format!("openat {name}: path escapes from parent"));
    }
    let full = root_dir().join(p);
    if !full.exists() {
        return Err(format!("openat {name}: no such file or directory"));
    }
    Ok(yutu_api::Media::File(full))
}

/// Opens a file for writing under the root, refusing blocked extensions so a
/// download can never overwrite a token or secret (divergence E1).
pub fn create_write(name: &str) -> Result<std::fs::File, String> {
    let ext = Path::new(name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
        .unwrap_or_default();
    if crate::ops::common::BLOCKED_EXTENSIONS.contains(&ext.as_str()) {
        return Err("access denied: blocked file extension".into());
    }
    let p = Path::new(name);
    if p.is_absolute() || p.components().any(|c| c == Component::ParentDir) {
        return Err(format!("openat {name}: path escapes from parent"));
    }
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(root_dir().join(p))
        .map_err(|e| {
            format!(
                "openat {name}: {}",
                e.to_string()
                    .split(" (os error")
                    .next()
                    .unwrap_or("")
                    .to_lowercase()
            )
        })
}
