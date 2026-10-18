//! Divergence E16: the agent on the Claude Code and Codex CLIs. A fake CLI
//! records how it was run and answers from scripted replies, so these tests
//! see exactly what reaches the CLI and what its replies make the agent do.

mod common;

use common::{block_on, cli_agent, recorder};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// A temporary fake CLI whose replies are `replies`, in order.
struct Fake {
    dir: PathBuf,
}

impl Fake {
    fn new(name: &str, replies: &[&str]) -> Fake {
        let dir =
            std::env::temp_dir().join(format!("youtube-fakecli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (i, r) in replies.iter().enumerate() {
            std::fs::write(dir.join(format!("reply.{}", i + 1)), r).unwrap();
        }
        let script = format!(
            r#"#!/bin/sh
D='{}'
n=$(cat "$D/count" 2>/dev/null || echo 0); n=$((n+1)); echo $n > "$D/count"
printf '%s\n' "$@" > "$D/args.$n"
cat > "$D/prompt.$n"
pwd > "$D/cwd.$n"
env > "$D/env.$n"
if [ -f "$D/spawn" ]; then sleep 30 & echo $! > "$D/grandchild"; fi
if [ -f "$D/fail" ]; then echo "not signed in" >&2; exit 3; fi
if [ -f "$D/fail-json" ]; then printf '{{"type":"result","subtype":"success","is_error":true,"result":"Invalid API key"}}'; exit 1; fi
reply=$(cat "$D/reply.$n")
out=""; prev=""
for a in "$@"; do [ "$prev" = "-o" ] && out="$a"; prev="$a"; done
if [ -n "$out" ]; then printf '%s' "$reply" > "$out"
else printf '{{"type":"result","subtype":"success","is_error":false,"result":"","structured_output":%s}}' "$reply"; fi
"#,
            dir.display()
        );
        let bin = dir.join("cli");
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        Fake { dir }
    }

    fn bin(&self) -> String {
        self.dir.join("cli").display().to_string()
    }

    fn read(&self, what: &str, n: usize) -> String {
        std::fs::read_to_string(self.dir.join(format!("{what}.{n}"))).unwrap_or_default()
    }

    fn args(&self, n: usize) -> Vec<String> {
        self.read("args", n).lines().map(String::from).collect()
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const DELETE: &str =
    r#"{"text":"","calls":[{"name":"video-delete","args_json":"{\"ids\":[\"v1\"]}"}]}"#;
const DONE: &str = r#"{"text":"All done.","calls":[]}"#;

fn deletes(log: &common::Log) -> usize {
    log.lock()
        .unwrap()
        .iter()
        .filter(|(l, _)| l.starts_with("DELETE"))
        .count()
}

fn has_pair(args: &[String], flag: &str, value: &str) -> bool {
    args.windows(2).any(|w| w[0] == flag && w[1] == value)
}

#[test]
fn claude_code_runs_without_tools_and_a_decline_sends_nothing() {
    block_on(async {
        let fake = Fake::new("claude", &[DELETE, DONE]);
        let (youtube, log) = recorder().await;
        let agent = cli_agent("claude-code", &fake.bin(), &youtube).await;

        let transcript = common::console(&agent, &["delete video v1", "n"]).await;

        assert!(
            transcript.contains("Delete video(s): v1 [y/N] All done."),
            "{transcript}"
        );
        assert_eq!(deletes(&log), 0);
        let args = fake.args(1);
        assert_eq!(args[0], "-p");
        for (flag, value) in [
            ("--tools", ""),
            ("--output-format", "json"),
            ("--setting-sources", ""),
            ("--settings", r#"{"disableAllHooks":true}"#),
        ] {
            assert!(has_pair(&args, flag, value), "{flag} {value:?} in {args:?}");
        }
        for flag in [
            "--strict-mcp-config",
            "--no-session-persistence",
            "--json-schema",
        ] {
            assert!(args.iter().any(|a| a == flag), "{flag} in {args:?}");
        }
        assert!(
            !args.iter().any(|a| a == "--model"),
            "the CLI's own default model"
        );
        let env = fake.read("env", 1);
        assert!(env.lines().any(|l| l.starts_with("HOME=")), "{env}");
        assert!(
            !env.lines()
                .any(|l| l.starts_with("CARGO") || l.starts_with("YOUTUBE_CLI_")),
            "only allowlisted variables reach the CLI: {env}"
        );
        for var in [
            "DISABLE_TELEMETRY=1",
            "DISABLE_ERROR_REPORTING=1",
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1",
        ] {
            assert!(env.lines().any(|l| l == var), "{var}");
        }
        let cwd = fake.read("cwd", 1);
        assert!(cwd.trim_end().ends_with("/work"), "{cwd}");
        assert!(
            !Path::new(cwd.trim_end()).exists(),
            "the scratch directory is removed"
        );
        assert!(
            fake.read("args", 1).contains("### video-delete"),
            "the tool catalog is in the cached system prompt"
        );
        let first = fake.read("prompt", 1);
        assert!(!first.contains("### video-delete"));
        assert!(first.contains("USER: delete video v1"));
        let second = fake.read("prompt", 2);
        assert!(second.contains(r#"ASSISTANT CALLED video-delete with {"ids":["v1"]}"#));
        assert!(second.contains("RESULT OF video-delete: {\"error\":\"Tool execution failed. Details: canceled by user\"}"));
    })
}

#[test]
fn codex_runs_read_only_and_an_approved_delete_reaches_youtube() {
    block_on(async {
        let fake = Fake::new("codex", &[DELETE, DONE]);
        let (youtube, log) = recorder().await;
        let agent = cli_agent("codex", &fake.bin(), &youtube).await;

        let transcript = common::console(&agent, &["delete video v1", "y"]).await;

        assert!(transcript.contains("All done."), "{transcript}");
        assert_eq!(deletes(&log), 1);
        let args = fake.args(1);
        assert_eq!(args[0], "exec");
        for (flag, value) in [
            ("--sandbox", "read-only"),
            ("-c", "web_search=\"disabled\""),
            ("-c", "analytics.enabled=false"),
            ("-c", "shell_environment_policy.inherit=\"none\""),
            ("-c", "features.apps=false"),
            ("-c", "features.plugins=false"),
            ("-c", "features.multi_agent=false"),
            ("-c", "features.shell_tool=false"),
            ("-c", "features.code_mode_host=false"),
        ] {
            assert!(has_pair(&args, flag, value), "{flag} {value:?} in {args:?}");
        }
        for flag in [
            "--ephemeral",
            "--ignore-user-config",
            "--ignore-rules",
            "--skip-git-repo-check",
        ] {
            assert!(args.iter().any(|a| a == flag), "{flag} in {args:?}");
        }
        assert_eq!(
            args.last().map(String::as_str),
            Some("-"),
            "prompt on stdin"
        );
        let prompt = fake.read("prompt", 1);
        assert!(prompt.starts_with("You are the language model behind an assistant."));
        assert!(prompt.contains("USER: delete video v1"));
    })
}

#[test]
fn a_failing_cli_ends_the_turn_with_its_error() {
    block_on(async {
        let fake = Fake::new("fail", &[]);
        std::fs::write(fake.dir.join("fail"), "").unwrap();
        let (youtube, log) = recorder().await;
        let agent = cli_agent("claude-code", &fake.bin(), &youtube).await;

        let transcript = common::console(&agent, &["hi"]).await;

        assert!(
            transcript
                .contains("AGENT_ERROR: claude-code: exited with exit status: 3: not signed in"),
            "{transcript}"
        );
        assert!(log.lock().unwrap().is_empty());
    })
}

#[test]
fn keys_and_endpoints_are_refused_for_cli_providers() {
    for provider in ["claude-code", "codex"] {
        let opts = youtube_agent::Options {
            args: "console".into(),
            provider: provider.into(),
            model: String::new(),
            api_key: "k".into(),
            base_url: String::new(),
            instruction: String::new(),
        };
        let err = youtube_agent::model_config(&opts, &|_| None).err().unwrap();
        assert_eq!(
            err,
            format!(
                "{provider}: --api-key and --base-url do not apply; the CLI uses its own sign-in"
            )
        );
    }
}

#[test]
fn an_error_the_cli_reports_on_stdout_reaches_the_user() {
    block_on(async {
        let fake = Fake::new("failjson", &[]);
        std::fs::write(fake.dir.join("fail-json"), "").unwrap();
        let (youtube, _) = recorder().await;
        let agent = cli_agent("claude-code", &fake.bin(), &youtube).await;

        let transcript = common::console(&agent, &["hi"]).await;

        assert!(
            transcript
                .contains("AGENT_ERROR: claude-code: exited with exit status: 1: Invalid API key"),
            "{transcript}"
        );
    })
}

#[test]
fn processes_the_cli_starts_end_with_the_call() {
    block_on(async {
        let fake = Fake::new("spawn", &[DONE]);
        std::fs::write(fake.dir.join("spawn"), "").unwrap();
        let (youtube, _) = recorder().await;
        let agent = cli_agent("codex", &fake.bin(), &youtube).await;

        common::console(&agent, &["hi"]).await;

        let pid = fake.read("grandchild", 1);
        let pid = std::fs::read_to_string(fake.dir.join("grandchild")).unwrap_or(pid);
        let alive = std::process::Command::new("kill")
            .args(["-0", pid.trim()])
            .status()
            .unwrap()
            .success();
        assert!(!alive, "the CLI's background child {pid} was killed");
    })
}

#[test]
fn a_missing_cli_is_reported_before_the_session_starts() {
    let opts = youtube_agent::Options {
        args: "console".into(),
        provider: "codex".into(),
        model: String::new(),
        api_key: String::new(),
        base_url: String::new(),
        instruction: String::new(),
    };
    let env = |k: &str| (k == "YOUTUBE_CLI_CODEX_BIN").then(|| "/nonexistent/codex".to_string());
    let err = youtube_agent::model_config(&opts, &env).err().unwrap();
    assert_eq!(
        err,
        "codex: /nonexistent/codex not found; install the Codex CLI and sign in, or set YOUTUBE_CLI_CODEX_BIN to its path"
    );
}

#[test]
fn a_tool_name_with_control_characters_cannot_reach_the_terminal() {
    block_on(async {
        let bad = r#"{"text":"","calls":[{"name":"x\u001b]10;#000000\u0007","args_json":"nope"}]}"#;
        let fake = Fake::new("badname", &[bad]);
        let (youtube, _) = recorder().await;
        let agent = cli_agent("claude-code", &fake.bin(), &youtube).await;

        let transcript = common::console(&agent, &["hi"]).await;

        assert!(
            transcript
                .contains("AGENT_ERROR: claude-code: arguments for x\\u{1b}]10;#000000\\u{7}"),
            "{transcript:?}"
        );
        assert!(!transcript.contains("\u{1b}]10"), "{transcript:?}");
    })
}
