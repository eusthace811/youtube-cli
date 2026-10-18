# youtube-cli Setup (command line)

The `youtube` command is youtube-cli. This skill drives it through the shell; no MCP server is involved.

## Check that it is ready

```bash
youtube version                      # prints "youtube-cli v... <os>/<arch>" on stderr
youtube channel list --for mine      # prints the signed-in channel when credentials work
```

If `youtube` is not found, it is not installed or not on `PATH`. If the second command fails, see Troubleshooting below.

## Install

```bash
# From source (macOS or Linux)
cargo build --release --locked -p youtube
install -m 755 target/release/youtube ~/.local/bin/youtube

# Node.js
npm i -g @youtube-cli/youtube
```

## Credentials

The user creates a Google Cloud project with the **YouTube Data API v3** enabled and an OAuth client of type **Desktop app**, and downloads its JSON as `client_secret.json`. Keep it and the token in a private folder:

```bash
mkdir -p ~/.config/youtube-cli && chmod 700 ~/.config/youtube-cli
export YOUTUBE_CLI_CREDENTIAL="$HOME/.config/youtube-cli/client_secret.json"
export YOUTUBE_CLI_CACHE_TOKEN="$HOME/.config/youtube-cli/youtube.token.json"
export YOUTUBE_CLI_ROOT="$HOME"
```

These usually live in the user's shell profile (`~/.zshrc` or `~/.bashrc`).

## Sign in

`youtube auth` reads its two paths from its flags only (their defaults count as set), never from the variables:

```bash
youtube auth --credential "$YOUTUBE_CLI_CREDENTIAL" --cacheToken "$YOUTUBE_CLI_CACHE_TOKEN"
```

A browser opens for consent and the token is saved with mode 0600. Every other command refreshes the token by itself. Signing in needs the user at the browser: ask them to run it, and never try to complete consent yourself.

## Environment variables

| Variable | Meaning | Default |
|----------|---------|---------|
| `YOUTUBE_CLI_CREDENTIAL` | Path, Base64 or JSON of the OAuth client secret | `client_secret.json` |
| `YOUTUBE_CLI_CACHE_TOKEN` | Path, Base64 or JSON of the cached token | `youtube.token.json` |
| `YOUTUBE_CLI_ROOT` | Directory every file path must stay inside: credential, token, uploads, caption downloads | current directory |
| `YOUTUBE_CLI_LOG_LEVEL` | `DEBUG`, `INFO`, `WARN` or `ERROR` | `INFO` |

## Troubleshooting

| Message | Cause | Fix |
|---------|-------|-----|
| `failed to parse client secret ... illegal base64 data` | The credential path was not found, or lies outside `YOUTUBE_CLI_ROOT`, so it was read as inline Base64 | Check the path and that `YOUTUBE_CLI_ROOT` contains it; for `auth`, pass `--credential` |
| `operation not confirmed (pass confirm: true or rerun with --yes)` | A write ran without `--yes` in a shell that is not a terminal | Expected: ask the user, then rerun with `--yes` |
| `failed to get <resource>` right after creating it | YouTube serves a stale copy for a few seconds after a write | Wait a few seconds and retry |
| A browser window opens unexpectedly | The token expired and could not be refreshed (weekly while the Google app is in Testing) | Let the user complete the sign-in |
| `googleapi: Error 403: ... quotaExceeded` | The daily API quota is used up | Wait for the reset (midnight Pacific time) |
