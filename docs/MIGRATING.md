# Moving from yutu (Go) to yutu-rs

yutu-rs is a drop-in replacement for the Go `yutu` v0.11.0 binary. Every command, flag, default, MCP tool, MCP resource and agent mode keeps its name and behavior, and the output is byte-for-byte the same except for the deliberate changes listed below. `docs/PARITY.md` shows the comparison that backs this.

## What carries over unchanged

- **Your OAuth client** (`client_secret.json`) and **your token file** (`youtube.token.json`). yutu-rs reads and writes the token in exactly the bytes Go's `json.MarshalWrite` produces, so the same file works with both binaries in either direction.
- **Environment variables**: `YUTU_CREDENTIAL`, `YUTU_CACHE_TOKEN`, `YUTU_ROOT` and `YUTU_LOG_LEVEL` mean what they mean in Go, with the same precedence (a non-empty flag wins over the variable).
- **Credential and token arguments**: a file path under the root, a standard base64 string, or inline JSON, resolved in that order.
- **Scopes and redirect**: the same three YouTube scopes and the `http://localhost:8216` loopback redirect, so an OAuth client configured for Go keeps working.
- **MCP**: the server name, version string, instructions, all 74 tool names, descriptions, input schemas, defaults and annotations, the `i18n://hl` resource and its templates. A Claude Code or other MCP configuration only needs the binary path changed.
- **Scripts**: exit codes (0 and 1), the `Error: ...` and `Run '... -h' to see usage.` lines, JSON/YAML/table output and the human success lines, including Go's quirks such as `Video <id> deleted` without a trailing newline.

## What changes, on purpose (Appendix E of the plan)

| Id | Change | Why |
|---|---|---|
| E1 | `caption download` asks for confirmation (`--yes`, a terminal `y`, or MCP elicitation) and refuses to write `.json`, `.env`, `.key`, `.pem`, `.crt`, `.cert`, `.pfx`, `.p12` files or outside the root. | In Go it could overwrite your token or client secret without asking. Scripts must add `--yes`. |
| E2 | A login triggered while serving stdio MCP prints only to stderr and never reads stdin. | Go wrote the login hints into the JSON-RPC stream. |
| E3 | Every token refresh is saved to the token file atomically. | Go saved only refreshes done at startup. |
| E4 | `mcp --mode http` verifies bearer tokens by POSTing them to Google's tokeninfo and can restrict accepted OAuth clients with `YUTU_MCP_ALLOWED_CLIENT_IDS` (comma separated). | Go put the token in a URL and accepted any client. |
| E5 | Stdio MCP traffic is logged to stderr only at `YUTU_LOG_LEVEL=DEBUG`. | Go logged every frame, including your channel data, at every level. |
| E6 | `video://category/{hl}` returns categories localized to `{hl}`. | Go ignored `{hl}`. |
| E7 | Uploads above 16 MiB use resumable 8 MiB chunks streamed from disk, retried after interruptions. | Bounded memory and resumable large uploads. Smaller uploads send Go's exact multipart request. |
| E8 | The HTTP MCP server sets header, request and idle timeouts and rejects requests whose `Origin` is not loopback. | Blocks slow-client exhaustion and DNS rebinding from a browser. |
| E9 | The agent cannot approve its own writes: every non-read tool call waits for a human `y` on the console or approval in the web UI. | Prompt injection through comments or titles cannot delete or edit your channel. |
| E10 | `yutu agent` reads the API key from `GEMINI_API_KEY` / `GOOGLE_API_KEY` or `OPENAI_API_KEY` when `--api-key` is absent. | Keeps keys out of shell history. |
| E11 | Agent web launchers print a random bearer token at start and require it on every route. | Go's dev UI and API had no authentication. |
| E12 | `--otel_to_cloud` is accepted and ignored with a notice. | No telemetry code ships in yutu-rs. |
| E13 | The generated skill lists only real tools and marks `caption-download` as writing a local file. | Go listed three tools that do not exist. |
| E14 | The Docker image documents how to reach `mcp --mode http` from outside the container. | Go's image exposed a port it could not serve. |
| E15 | `install.sh` verifies the SHA-256 checksum and, with `gh` installed, the build-provenance attestation, and installs the agent skill only with `--with-skill`. | Go's installer did neither and always ran `npx --yes skills add`. |

## Things that behave as in Go, including known Go defects

yutu-rs keeps Go's behavior even where it is surprising, so scripts see no change. The Go defects reproduced on purpose are listed in `docs/PARITY.md` under "Known Go defects reproduced on purpose" (for example, `playlist update` from the CLI fails because Go never fetches the playlist it updates). They are candidates for fixes after cutover, not silent changes.

## No telemetry

yutu-rs contains no telemetry, analytics or crash reporting code. `deny.toml` bans the known telemetry crates and `xtask egress` checks every release binary for unexpected network hosts. The only hosts it talks to are Google's OAuth and YouTube endpoints and, for `yutu agent`, the LLM provider you choose.
