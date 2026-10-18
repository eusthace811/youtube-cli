//! Go's `path/filepath` lexical rules, so a path argument names the same file
//! under the root as it does for the Go binary. Slash-separated (Unix) only.

use std::io;

/// `filepath.Clean`.
pub fn clean(path: &str) -> String {
    let p = path.as_bytes();
    if p.is_empty() {
        return ".".to_string();
    }
    let rooted = p[0] == b'/';
    let n = p.len();
    let mut out: Vec<u8> = Vec::with_capacity(n);
    let (mut r, mut dotdot) = (0, 0);
    if rooted {
        out.push(b'/');
        r = 1;
        dotdot = 1;
    }
    while r < n {
        if p[r] == b'/' || (p[r] == b'.' && (r + 1 == n || p[r + 1] == b'/')) {
            r += 1;
        } else if p[r] == b'.' && p[r + 1] == b'.' && (r + 2 == n || p[r + 2] == b'/') {
            r += 2;
            if out.len() > dotdot {
                let mut w = out.len() - 1;
                while w > dotdot && out[w] != b'/' {
                    w -= 1;
                }
                out.truncate(w);
            } else if !rooted {
                if !out.is_empty() {
                    out.push(b'/');
                }
                out.extend_from_slice(b"..");
                dotdot = out.len();
            }
        } else {
            if (rooted && out.len() != 1) || (!rooted && !out.is_empty()) {
                out.push(b'/');
            }
            while r < n && p[r] != b'/' {
                out.push(p[r]);
                r += 1;
            }
        }
    }
    if out.is_empty() {
        out.push(b'.');
    }
    String::from_utf8(out).expect("cleaning splits only at ASCII bytes")
}

/// `filepath.Abs`.
pub fn abs(path: &str) -> io::Result<String> {
    if path.starts_with('/') {
        return Ok(clean(path));
    }
    Ok(clean(&format!("{}/{}", getwd()?, path)))
}

/// `filepath.Rel`; `None` where Go returns an error.
pub fn rel(base: &str, targ: &str) -> Option<String> {
    let base = clean(base);
    let targ = clean(targ);
    if targ == base {
        return Some(".".to_string());
    }
    let base = if base == "." { String::new() } else { base };
    if base.starts_with('/') != targ.starts_with('/') {
        return None;
    }
    let (b, t) = (base.as_bytes(), targ.as_bytes());
    let (bl, tl) = (b.len(), t.len());
    let (mut b0, mut bi, mut t0, mut ti) = (0, 0, 0, 0);
    loop {
        while bi < bl && b[bi] != b'/' {
            bi += 1;
        }
        while ti < tl && t[ti] != b'/' {
            ti += 1;
        }
        if t[t0..ti] != b[b0..bi] {
            break;
        }
        if bi < bl {
            bi += 1;
        }
        if ti < tl {
            ti += 1;
        }
        b0 = bi;
        t0 = ti;
    }
    if &b[b0..bi] == b".." {
        return None;
    }
    if b0 != bl {
        let seps = b[b0..bl].iter().filter(|&&c| c == b'/').count();
        let mut buf = String::from("..");
        for _ in 0..seps {
            buf.push_str("/..");
        }
        if t0 != tl {
            buf.push('/');
            buf.push_str(&targ[t0..]);
        }
        return Some(buf);
    }
    Some(targ[t0..].to_string())
}

/// `filepath.Ext`.
pub fn ext(path: &str) -> &str {
    for (i, c) in path.bytes().enumerate().rev() {
        match c {
            b'/' => break,
            b'.' => return &path[i..],
            _ => {}
        }
    }
    ""
}

/// `os.Getwd`: `$PWD` when it names the current directory, else the kernel's
/// answer, so a symlinked working directory resolves like it does in Go.
pub fn getwd() -> io::Result<String> {
    let dot = std::fs::metadata(".")?;
    if let Ok(pwd) = std::env::var("PWD")
        && pwd.starts_with('/')
        && let Ok(m) = std::fs::metadata(&pwd)
        && same_file(&m, &dot)
    {
        return Ok(pwd);
    }
    std::env::current_dir()?
        .into_os_string()
        .into_string()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "working directory is not UTF-8"))
}

#[cfg(unix)]
fn same_file(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev() && a.ino() == b.ino()
}

#[cfg(not(unix))]
fn same_file(_: &std::fs::Metadata, _: &std::fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_matches_go() {
        for (input, want) in [
            ("", "."),
            ("/", "/"),
            ("/..", "/"),
            ("/../a", "/a"),
            ("a/../..", ".."),
            ("../../a/b/../c", "../../a/c"),
            ("a//b/./c/", "a/b/c"),
            ("/a/b/../../..", "/"),
            ("abc/def/../../..", ".."),
            ("./", "."),
            ("/a/./b/../c", "/a/c"),
        ] {
            assert_eq!(clean(input), want, "clean({input:?})");
        }
    }

    #[test]
    fn rel_matches_go() {
        for (base, targ, want) in [
            ("/a/b", "/a/b", Some(".")),
            ("/a/b", "/a/b/c.json", Some("c.json")),
            ("/a/b", "/a/c/d", Some("../c/d")),
            ("/a/b", "/x", Some("../../x")),
            ("/", "/etc/hosts", Some("etc/hosts")),
            ("/a/b", "/a", Some("..")),
            (".", "/a", None),
            ("rel", "/a", None),
        ] {
            assert_eq!(rel(base, targ).as_deref(), want, "rel({base:?}, {targ:?})");
        }
    }

    #[test]
    fn ext_matches_go() {
        assert_eq!(ext("a/b.JSON"), ".JSON");
        assert_eq!(ext("a.b/c"), "");
        assert_eq!(ext(".env"), ".env");
        assert_eq!(ext("x.json/"), "");
    }
}
