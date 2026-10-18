# youtube-cli

A CLI, MCP server and AI agent for YouTube, installed as the `youtube` command. It is a Rust rewrite of [yutu](https://github.com/eat-pray-ai/yutu) v0.11.0 and behaves like it command for command, apart from the program's name and the deliberate fixes listed in `docs/MIGRATING.md` (`docs/PARITY.md`). It has no telemetry. It starts in about 3.5 ms where Go takes about 12 ms, and the binary is about 7 MB where Go's is 57 MB (`docs/PERF.md`).

Every write to your channel needs your approval: `--yes` or a `y` typed at a terminal on the CLI, an accepted elicitation over MCP, and a human `y` or browser click in the agent (`docs/SECURITY.md`).

## Install

macOS and Linux, on amd64 and arm64. Windows is not supported yet; keep Go yutu there.

From a release, with the checksum and build-provenance checks in `install.sh`:

```shell
curl -sSfL https://raw.githubusercontent.com/OWNER/REPO/main/install.sh | YOUTUBE_CLI_REPO=OWNER/REPO bash   # OWNER/REPO: your youtube-cli repository
```

From source, with the Rust toolchain that `rust-toolchain.toml` pins:

```shell
cargo build --release --locked -p youtube
cp target/release/youtube ~/.local/bin/youtube
```

Both install the program as `youtube`. It does not clash with Go yutu, which installs as `yutu`. Its environment variables use the `YOUTUBE_CLI_` prefix where yutu used `YUTU_`; the files they point to are unchanged.

## Connect your YouTube account

1. In the Google Cloud console, create a project, enable the **YouTube Data API v3**, and create an **OAuth client ID** of type *Desktop app*. Download its JSON as `client_secret.json`. While the consent screen's publishing status is *Testing*, Google expires the sign-in after 7 days, so expect to run `youtube auth` again weekly, or set it to *In production*. Videos uploaded through a project Google has not audited are locked to private, whatever `--privacy` says.
2. Sign in once. A browser opens for consent and the token is saved with mode 0600:

   ```shell
   youtube auth --credential client_secret.json --cacheToken youtube.token.json
   ```

Paths resolve from the current directory, as in Go. To use youtube from anywhere, give absolute paths in the environment:

```shell
export YOUTUBE_CLI_CREDENTIAL="$HOME/.config/youtube/client_secret.json"
export YOUTUBE_CLI_CACHE_TOKEN="$HOME/.config/youtube/youtube.token.json"
export YOUTUBE_CLI_ROOT="$HOME"
```

`youtube auth` takes its two paths from `--credential` and `--cacheToken` only, because their defaults count as set (as in Go). Every other command reads the environment variables above.

`YOUTUBE_CLI_ROOT` is the directory every file path must stay inside: the credential and token as well as uploads and caption downloads. A path outside it is read as inline Base64 or JSON instead, which fails with "illegal base64 data". A token file written by Go yutu works unchanged, and so does one written by youtube-cli for Go.

## Use it

```shell
youtube channel list --for mine
youtube playlist list --mine
youtube playlist update --id PL... --title "New title"     # asks before writing
youtube video delete --ids VIDEO_ID --yes                  # --yes skips the question
```

`youtube --help` lists all 35 commands, and `youtube <command> --help` shows each one's flags.

## MCP server

```shell
claude mcp add youtube -- youtube mcp          # stdio, for Claude Code
youtube mcp --mode http                     # streamable HTTP at http://localhost:8216/mcp
```

The HTTP mode needs a Google bearer token on every request. Set `YOUTUBE_CLI_MCP_ALLOWED_CLIENT_IDS` to accept only your own OAuth clients. To run it in a container, see `dist/README.md`.

## Agent

```shell
youtube agent --provider claude-code -m sonnet   # your signed-in Claude Code
youtube agent --provider codex                   # your signed-in Codex CLI
youtube agent --api-key "$GEMINI_API_KEY"        # Gemini (the default provider)
youtube agent --provider openai --model gpt-5.6-terra --api-key "$OPENAI_API_KEY"
```

The Claude Code and Codex providers need no API key. youtube runs the CLI only as a model, with its own tools, connectors and plugins off. `YOUTUBE_CLI_CLAUDE_BIN` or `YOUTUBE_CLI_CODEX_BIN` can point at the executable.

The agent asks `Tool call needs approval: ... [y/N]` before every change. Only a `y` typed at a terminal after the question appears counts.

For the dev web UI, run `youtube agent --args "web webui api"` and open the sign-in link printed at start.

## Skills for Claude

Two skills ship in `skills/`, generated from the same command registry as the program:

- `skills/youtube-cli` drives the `youtube` command in the shell. It holds a reference of every command and flag, and rules for a live channel: Claude reads freely, shows you the exact command for any change, and adds `--yes` only after you agree.
- `skills/youtube` drives the MCP tools, for use with `youtube mcp`.

Install the one you use, not both:

```shell
ln -sfn "$PWD/skills/youtube-cli" ~/.claude/skills/youtube-cli   # command line (a symlink follows regenerations)
cp -R skills/youtube ~/.claude/skills/youtube           # MCP
```

Restart Claude Code after installing.

## Documentation

- `docs/MIGRATING.md`: moving from Go yutu, including every intentional difference.
- `docs/SECURITY.md`: the threat model, the reviews, and the accepted risks.
- `docs/PARITY.md`: the parity report against Go across 2,831 cases.
- `docs/PERF.md`: measured performance against Go.
- `dist/README.md`: the installer, npm, Docker and MCP-registry packaging.

## License

Apache-2.0, as is Go yutu. See `LICENSE` and `NOTICE`.
