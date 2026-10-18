//! Writes generated files, or with `--check` fails when any is stale.

use std::fs;
use std::path::PathBuf;

pub struct Generated {
    /// Path relative to the repo root.
    pub path: &'static str,
    pub text: String,
    pub executable: bool,
}

pub fn emit(files: &[Generated], args: &[String], command: &str) -> Result<(), String> {
    let root = crate::repo_root();
    if args.iter().any(|a| a == "--check") {
        let stale: Vec<&str> = files
            .iter()
            .filter(|f| fs::read_to_string(root.join(f.path)).ok().as_deref() != Some(&f.text))
            .map(|f| f.path)
            .collect();
        if !stale.is_empty() {
            return Err(format!(
                "stale generated files: {}\nrun `cargo run -p xtask -- {command}`",
                stale.join(", ")
            ));
        }
        println!("{command}: {} files up to date", files.len());
        return Ok(());
    }
    for f in files {
        let path: PathBuf = root.join(f.path);
        fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::write(&path, &f.text).map_err(|e| format!("{}: {e}", path.display()))?;
        #[cfg(unix)]
        if f.executable {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
                .map_err(|e| e.to_string())?;
        }
        println!("wrote {}", f.path);
    }
    Ok(())
}
