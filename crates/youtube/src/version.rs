//! `youtube version`: the build banner, on stderr as Go's was.

const VERSION: &str = env!("YOUTUBE_CLI_VERSION");
const COMMIT_DATE: &str = env!("YOUTUBE_CLI_COMMIT_DATE");
const BUILDER: &str = env!("YOUTUBE_CLI_BUILDER");

/// GOOS for this build.
fn go_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        os => os,
    }
}

/// GOARCH for this build.
fn go_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        "x86" => "386",
        arch => arch,
    }
}

pub fn banner() -> String {
    format!(
        "youtube-cli {VERSION} {}/{}\nCLI, MCP server, and AI agent to automate YouTube workflows\nbuild: {BUILDER}-{COMMIT_DATE}\n",
        go_os(),
        go_arch()
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_banner_names_the_program_and_build() {
        let b = super::banner();
        assert!(
            b.starts_with(&format!("youtube-cli {} ", super::VERSION)),
            "{b}"
        );
        assert!(
            b.ends_with(&format!(
                "build: {}-{}\n",
                super::BUILDER,
                super::COMMIT_DATE
            )),
            "{b}"
        );
        assert!(!b.contains("eat-pray-ai"), "{b}");
    }
}
