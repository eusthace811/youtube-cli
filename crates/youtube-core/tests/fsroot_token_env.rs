//! Its own test binary: it sets process environment the other fsroot tests
//! must not see.

#[test]
fn a_token_named_by_an_absolute_path_is_protected() {
    let dir = std::env::temp_dir().join(format!("youtube-fsroot-abs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::fs::write(dir.join("tok"), "{}").unwrap();
    // SAFETY: the only test in this binary, so no other thread reads the
    // environment.
    unsafe { std::env::set_var("YOUTUBE_CLI_CACHE_TOKEN", dir.join("tok")) };
    let root = youtube_core::fsroot::Root::open(dir.to_str().unwrap()).unwrap();
    assert_eq!(
        root.create_write("tok").unwrap_err().to_string(),
        "access denied: blocked file extension"
    );
    assert_eq!(std::fs::read_to_string(dir.join("tok")).unwrap(), "{}");
    std::fs::remove_dir_all(&dir).unwrap();
}
