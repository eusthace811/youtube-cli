# yutu-rs

A Rust rewrite of [yutu](https://github.com/eat-pray-ai/yutu) v0.11.0, the CLI, MCP server and AI agent for YouTube. It behaves like Go yutu, command for command and byte for byte (`docs/PARITY.md`), with deliberate fixes listed in `docs/MIGRATING.md`. It has no telemetry. It starts in about 3.5 ms where Go takes about 12 ms, and the binary is about 7 MB where Go's is 57 MB (`docs/PERF.md`).

Every write to your channel needs your approval: `--yes` or a `y` typed at a terminal on the CLI, an accepted elicitation over MCP, and a human `y` or browser click in the agent (`docs/SECURITY.md`).

## Install

macOS and Linux, on amd64 and arm64. Windows is not supported yet; keep Go yutu there.

From a release, with the checksum and build-provenance checks in `install.sh`:

```shell
curl -sSfL https://raw.githubusercontent.com/OWNER/REPO/main/install.sh | YUTU_REPO=OWNER/REPO bash
```

From source, with the Rust toolchain that `rust-toolchain.toml` pins:

```shell
cargo build --release --locked -p yutu
cp target/release/yutu ~/.local/bin/yutu
```

Both install the program as `yutu`, the same name as Go yutu. Remove Go yutu from your `PATH`, or put yutu-rs ahead of it. `yutu version` prints the same banner for both.

## Connect your YouTube account

1. In the Google Cloud console, create a project, enable the **YouTube Data API v3**, and create an **OAuth client ID** of type *Desktop app*. Download its JSON as `client_secret.json`.
2. Sign in once. A browser opens for consent and the token is saved with mode 0600:

   ```shell
   yutu auth --credential client_secret.json --cacheToken youtube.token.json
   ```

Paths resolve from the current directory, as in Go. To use yutu from anywhere, give absolute paths in the environment:

```shell
export YUTU_CREDENTIAL="$HOME/.config/yutu/client_secret.json"
export YUTU_CACHE_TOKEN="$HOME/.config/yutu/youtube.token.json"
export YUTU_ROOT="$HOME"
```

`YUTU_ROOT` is the directory files you name for uploads and caption downloads must stay inside. A token file written by Go yutu works unchanged, and so does one written by yutu-rs for Go.

## Use it

```shell
yutu channel list --for mine
yutu playlist list --mine
yutu playlist update --id PL... --title "New title"     # asks before writing
yutu video delete --ids VIDEO_ID --yes                  # --yes skips the question
```

`yutu --help` lists all 35 commands, and `yutu <command> --help` shows each one's flags.

## MCP server

```shell
claude mcp add yutu -- yutu mcp          # stdio, for Claude Code
yutu mcp --mode http                     # streamable HTTP at http://localhost:8216/mcp
```

The HTTP mode needs a Google bearer token on every request. Set `YUTU_MCP_ALLOWED_CLIENT_IDS` to accept only your own OAuth clients. To run it in a container, see `dist/README.md`.

## Agent

```shell
yutu agent --provider claude-code -m sonnet   # your signed-in Claude Code
yutu agent --provider codex                   # your signed-in Codex CLI
yutu agent --api-key "$GEMINI_API_KEY"        # Gemini (the default provider)
yutu agent --provider openai --model gpt-5.6-terra --api-key "$OPENAI_API_KEY"
```

The Claude Code and Codex providers need no API key. yutu runs the CLI only as a model, with its own tools, connectors and plugins off. `YUTU_CLAUDE_BIN` or `YUTU_CODEX_BIN` can point at the executable.

The agent asks `Tool call needs approval: ... [y/N]` before every change. Only a `y` typed at a terminal after the question appears counts.

For the dev web UI, run `yutu agent --args "web webui api"` and open the sign-in link printed at start.

## Documentation

- `docs/MIGRATING.md`: moving from Go yutu, including every intentional difference.
- `docs/SECURITY.md`: the threat model, the reviews, and the accepted risks.
- `docs/PARITY.md`: the parity report against Go across 2,831 cases.
- `docs/PERF.md`: measured performance against Go.
- `dist/README.md`: the installer, npm, Docker and MCP-registry packaging.

## License

Apache-2.0, as is Go yutu. See `LICENSE` and `NOTICE`.
