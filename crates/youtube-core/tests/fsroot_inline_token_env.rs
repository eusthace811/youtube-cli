//! Its own test binary: it sets process environment the other fsroot tests
//! must not see.

#[test]
fn an_inline_token_does_not_block_downloads() {
    let dir = std::env::temp_dir().join(format!("youtube-fsroot-inline-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // SAFETY: the only test in this binary, so no other thread reads the
    // environment.
    unsafe { std::env::set_var("YOUTUBE_CLI_CACHE_TOKEN", "eyJ0b2tlbiI6".repeat(40)) };
    let root = youtube_core::fsroot::Root::open(dir.to_str().unwrap()).unwrap();
    assert!(root.create_write("ok.srt").is_ok());
    std::fs::remove_dir_all(&dir).unwrap();
}
