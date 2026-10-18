<!-- Keep in sync with README.md Prerequisites and Installation sections. -->

# Youtube Setup Guide

Before using any youtube skill, you need to install `youtube` and configure YouTube API credentials.

## Prerequisites

An account on [Google Cloud Platform](https://console.cloud.google.com/) is required. Set up the following:

1. **Create a GCP Project** and enable these APIs under `APIs & Services -> Enable APIs and services`:
   - [YouTube Data API v3](https://console.cloud.google.com/apis/api/youtube.googleapis.com/overview) (Required)
   - [YouTube Analytics API](https://console.cloud.google.com/apis/api/youtubeanalytics.googleapis.com/overview) (Optional)
   - [YouTube Reporting API](https://console.cloud.google.com/apis/api/youtubereporting.googleapis.com/overview) (Optional)

2. **Create OAuth credentials**:
   - Go to `APIs & Services -> OAuth consent screen`, create a consent screen with yourself as a test user
   - Go to `Credentials -> Create Credentials -> OAuth Client ID`, select `Desktop app`
   - Download the credential file and save it as `client_secret.json`

3. **Authenticate**:

   ```bash
   youtube auth --credential client_secret.json
   ```

   A browser window will open for you to grant YouTube access. After granting permission, a token is saved to `youtube.token.json`.

## Installation

Install `youtube` (youtube-cli, macOS or Linux) using one of these methods:

```bash
# Node.js
npm i -g @youtube-cli/youtube

# Shell script, with checksum and provenance checks (OWNER/REPO: the youtube-cli repository)
curl -sSfL https://raw.githubusercontent.com/OWNER/REPO/main/install.sh | YOUTUBE_CLI_REPO=OWNER/REPO bash

# From source
cargo build --release --locked -p youtube
```

## MCP Server

Add `youtube` as an MCP server after installation and authentication:

```bash
# Claude Code
claude mcp add -e YOUTUBE_CLI_CREDENTIAL=/absolute/path/to/client_secret.json \
  -e YOUTUBE_CLI_CACHE_TOKEN=/absolute/path/to/youtube.token.json \
  youtube -- youtube mcp

# Codex
codex mcp add --env YOUTUBE_CLI_CREDENTIAL=/absolute/path/to/client_secret.json \
  --env YOUTUBE_CLI_CACHE_TOKEN=/absolute/path/to/youtube.token.json \
  youtube -- youtube mcp
```

For VS Code, Cursor, or other tools, add to your MCP settings:

```json
{
  "youtube": {
    "type": "stdio",
    "command": "youtube",
    "args": ["mcp"],
    "env": {
      "YOUTUBE_CLI_CREDENTIAL": "/absolute/path/to/client_secret.json",
      "YOUTUBE_CLI_CACHE_TOKEN": "/absolute/path/to/youtube.token.json"
    }
  }
}
```

## Skills

```bash
npx skills add OWNER/REPO --skill youtube   # OWNER/REPO: the youtube-cli repository
```

## Environment Variables

| Variable | Description                                  | Default |
|----------|----------------------------------------------|---------|
| `YOUTUBE_CLI_CREDENTIAL` | Path, Base64, or JSON of OAuth client secret | `client_secret.json` |
| `YOUTUBE_CLI_CACHE_TOKEN` | Path, Base64, or JSON of cached OAuth token  | `youtube.token.json` |
| `YOUTUBE_CLI_ROOT` | Root directory for file resolution           | Current working directory |
| `YOUTUBE_CLI_LOG_LEVEL` | Log level: `DEBUG`, `INFO`, `WARN`, `ERROR`  | `INFO` |

For more details, see the youtube-cli README.
