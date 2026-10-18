//! Lists the embedded dev UI bundle, assets/webui/distr, as a table of
//! `(path, bytes)` for `launch::webui`.

use std::path::{Path, PathBuf};

fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, PathBuf)>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .expect("assets/webui/distr")
        .map(|e| e.expect("dir entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            walk(&path, root, out);
        } else {
            let rel = path.strip_prefix(root).expect("under root");
            let rel = rel.to_str().expect("UTF-8 path").replace('\\', "/");
            out.push((rel, path));
        }
    }
}

fn main() {
    // Read at run time: a build script binary reused from another checkout
    // must not point at that checkout's files.
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets it"));
    let root = manifest.join("assets/webui/distr");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    walk(&root, &root, &mut files);
    let mut src = String::from("pub static FILES: &[(&str, &[u8])] = &[\n");
    for (rel, path) in files {
        src.push_str(&format!(
            "    ({rel:?}, include_bytes!({:?})),\n",
            path.display().to_string()
        ));
    }
    src.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets it")).join("webui.rs");
    std::fs::write(out, src).expect("write webui.rs");

    // `/version` reports the compiler as ADK reports the Go runtime.
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let version = std::process::Command::new(rustc)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|v| v.split_whitespace().nth(1).map(String::from))
        .unwrap_or_default();
    println!("cargo:rustc-env=YUTU_RUSTC_VERSION={version}");
}
