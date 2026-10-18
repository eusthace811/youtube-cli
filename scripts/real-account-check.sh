#!/usr/bin/env bash
# Checks youtube-cli against a real YouTube account, touching only a
# private test playlist it creates and deletes. Run it yourself; it signs
# in through your browser on first use.
#
# Usage: scripts/real-account-check.sh [path/to/youtube]
# Needs YOUTUBE_CLI_CREDENTIAL (client_secret.json) and optionally
# YOUTUBE_CLI_CACHE_TOKEN, as in the README.
set -euo pipefail

BIN="${1:-target/release/youtube}"
step() { printf '\n== %s\n' "$*"; }

step "sign-in (opens a browser if there is no token yet)"
"${BIN}" channel list --for mine -o table

step "read: your playlists"
"${BIN}" playlist list --mine -o table

step "write: create a private test playlist"
created="$("${BIN}" playlist insert --title "youtube-cli check $(date +%s)" \
  --privacy private --channelId "$("${BIN}" channel list --for mine -o json |
    python3 -c 'import json,sys; print(json.load(sys.stdin)[0]["id"])')" -o json --yes)"
id="$(printf '%s' "${created}" | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')"
echo "created ${id}"

step "update (E18): rename it"
"${BIN}" playlist update --id "${id}" --title "youtube-cli check (renamed)" --yes

step "confirm the new title"
"${BIN}" playlist list --ids "${id}" -o table

step "delete it"
"${BIN}" playlist delete --ids "${id}" --yes

step "all steps succeeded"
