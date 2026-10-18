use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use yutu_core::fsroot::Root;

/// `<tmp>/yutu-fsroot-<rand>/root` with `x` beside the root, outside it.
struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Fixture {
        let mut name = [0u8; 8];
        getrandom_bytes(&mut name);
        let hex: String = name.iter().map(|b| format!("{b:02x}")).collect();
        let base = std::env::temp_dir().join(format!("yutu-fsroot-{hex}"));
        std::fs::create_dir_all(base.join("root")).unwrap();
        std::fs::write(base.join("x"), "outside").unwrap();
        Fixture(base.canonicalize().unwrap())
    }

    fn root_path(&self) -> PathBuf {
        self.0.join("root")
    }

    fn root(&self) -> Root {
        Root::open(self.root_path().to_str().unwrap()).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn getrandom_bytes(buf: &mut [u8]) {
    let mut f = std::fs::File::open("/dev/urandom").unwrap();
    f.read_exact(buf).unwrap();
}

fn read_err(root: &Root, name: &str) -> String {
    root.open_read(name).unwrap_err().to_string()
}

#[test]
fn reads_cannot_leave_the_root() {
    let fx = Fixture::new();
    let root = fx.root();
    assert_eq!(
        read_err(&root, "../x"),
        "openat ../x: path escapes from parent"
    );
    std::os::unix::fs::symlink("/etc/hosts", fx.root_path().join("hosts")).unwrap();
    assert_eq!(
        read_err(&root, "hosts"),
        "openat hosts: path escapes from parent"
    );
    std::os::unix::fs::symlink("../x", fx.root_path().join("up")).unwrap();
    assert_eq!(read_err(&root, "up"), "openat up: path escapes from parent");
    assert_eq!(
        read_err(&root, "missing.mp4"),
        "openat missing.mp4: no such file or directory"
    );
}

#[test]
fn reads_stay_possible_inside_the_root() {
    let fx = Fixture::new();
    std::fs::create_dir(fx.root_path().join("sub")).unwrap();
    std::fs::write(fx.root_path().join("video.mp4"), "frames").unwrap();
    std::os::unix::fs::symlink("../video.mp4", fx.root_path().join("sub/link.mp4")).unwrap();
    let root = fx.root();
    let mut text = String::new();
    root.open_read("sub/link.mp4")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    assert_eq!(text, "frames");
}

#[test]
fn secrets_are_refused_for_media_reads() {
    let fx = Fixture::new();
    std::fs::write(fx.root_path().join("client_secret.json"), "{}").unwrap();
    let root = fx.root();
    for name in [
        "client_secret.json",
        "YOUTUBE.TOKEN.JSON",
        "a.env",
        "k.Pem",
        "c.p12",
        "sub/.key",
    ] {
        assert_eq!(
            read_err(&root, name),
            "access denied: blocked file extension",
            "{name}"
        );
    }
}

#[test]
fn writes_refuse_secrets_and_escapes() {
    let fx = Fixture::new();
    let token = fx.root_path().join("youtube.token.json");
    std::fs::write(&token, "TOKEN").unwrap();
    let root = fx.root();
    let write_err = |name: &str| root.create_write(name).unwrap_err().to_string();
    assert_eq!(
        write_err("youtube.token.json"),
        "access denied: blocked file extension"
    );
    assert_eq!(std::fs::read_to_string(&token).unwrap(), "TOKEN");
    assert_eq!(
        write_err("../x.srt"),
        "openat ../x.srt: path escapes from parent"
    );
    assert_eq!(std::fs::read_to_string(fx.0.join("x")).unwrap(), "outside");
    assert_eq!(
        write_err("new/dir/cap.srt"),
        "openat new/dir/cap.srt: no such file or directory"
    );
}

#[test]
fn writes_truncate_at_mode_0600() {
    let fx = Fixture::new();
    let root = fx.root();
    root.create_write("cap.srt")
        .unwrap()
        .write_all(b"long caption")
        .unwrap();
    root.create_write("cap.srt")
        .unwrap()
        .write_all(b"short")
        .unwrap();
    let path = fx.root_path().join("cap.srt");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "short");
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}
