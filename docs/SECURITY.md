# Security

youtube-cli drives a YouTube channel with OAuth tokens that hold the full `youtube`, `youtube.force-ssl` and `youtube.channel-memberships.creator` scopes. Whoever holds the token can edit, delete and upload on the channel. The design goal is that nothing but the operator's explicit action can use it, and that it never leaves the machine except to Google.

## Threat model

| Asset | Threat | Control |
|---|---|---|
| OAuth refresh and access tokens | Leaking through logs, errors, URLs, MCP output or a third-party host | Tokens live in a redacting `Secret` type. The token file is written atomically with mode 0600. Tokeninfo verification POSTs the token (E4). Resumable upload sessions must stay on the upload host or a TLS `*.googleapis.com` host. The parity endpoint override exists only in test builds and only for plain loopback URLs, and CI's `xtask egress --release` fails any release binary that still carries a `YUTU_PARITY_` hook. |
| The channel | Writes the operator did not intend, including through prompt injection in the agent or a malicious MCP client | Every write, destructive and local-file operation needs `--yes`, a `y` typed at a terminal, MCP elicitation acceptance, or a human approval in the agent (E9). The agent cannot approve its own calls. Its console takes approvals only from a terminal, only from a line typed after the question is shown, and never with `CI` set. `caption download` is gated too (E1). |
| Local files | Reading secrets for upload, or overwriting the token or client secret | Every file a user names opens inside the root (`YOUTUBE_CLI_ROOT` or the working directory) through cap-std, refusing `..`, absolute paths and escaping symlinks. `.json .env .key .pem .crt .cert .pfx .p12` and well-known secret names are refused for uploads and caption downloads. The active token and credential are also refused by identity (device and inode), however they are named, and the check runs on the opened handle before any truncation (E1). |
| Local network listeners | Other local processes or web pages driving the account | The OAuth callback listens on loopback only for the duration of a login. HTTP MCP verifies Google bearer tokens, rejects non-loopback `Origin` headers and sets timeouts (E8). Agent web launchers print two per-launch tokens: a bearer token for programs and a sign-in token for the dev UI's browser. Neither works in place of the other, only the browser can approve writes, and browser requests get Origin and Host checks on every route (E11). |
| LLM API keys | Leaking to a third party | Keys go only to the configured provider. The LLM clients do not follow redirects, so a redirect cannot carry the key header to another host. |
| The Claude Code and Codex backends (E16) | A prompt-injected model acting through the CLI's own tools, connectors or environment instead of youtube's gated tools | youtube runs the CLI only as a model: the conversation goes in on stdin, a reply comes back in a fixed JSON schema, and tool calls return as data that youtube's approval gate handles. Claude Code runs with every built-in tool disabled, no MCP servers, no user settings or hooks and no saved session. Codex runs with its connectors (ChatGPT apps such as Gmail, GitHub and Drive), plugins, shell, browsers, image tools and web search switched off, user config and rules ignored, a read-only sandbox and no saved session. Its code-mode tool fails closed with the host disabled, and its sub-agent tools, which Codex's model catalog adds regardless of configuration, carry the same restrictions; real-model checks confirmed it can then read no file, environment value or network resource. Each call runs in a fresh empty directory that is deleted afterwards, in its own process group that is killed when the call ends, with an environment limited to the basic process variables and the vendor's own (no `YOUTUBE_CLI_*` values). Both get their vendors' telemetry opt-outs. |
| The operator's privacy | Telemetry, analytics, update checks | None ship. `deny.toml` bans the known telemetry, analytics, crash-reporting and update-check crates and OpenSSL. `xtask egress` fails the build if the release binary contains a URL for any host outside Google's OAuth and YouTube APIs and the two LLM providers. R12 lane 5 runs the whole corpus with only loopback networking allowed. |
| The supply chain | Compromised or unexpected dependencies, tampered releases | crates.io only. `cargo deny` (advisories, bans, licenses, sources) and `cargo audit` run in CI, builds use `--locked`, and CI actions are pinned by commit SHA. Releases ship checksums, a CycloneDX SBOM and GitHub build-provenance attestations. `install.sh` verifies the checksum and the attestation (E15). |

## Deliberate fixes of Go defects (Appendix E)

`docs/MIGRATING.md` has the user-facing table, including the correctness fixes E17 (`max_results` 0) and E18 (updates that always failed). `parity/divergences.toml` is the registry the differ enforces, and its `proof` field names each test.

| Id | Fix | Proof |
|---|---|---|
| E1 | `caption download` is gated and writes only through the sandbox, never over a token or secret | `crates/youtube-core/tests/fsroot.rs`, `crates/youtube-core/tests/fsroot_token_env.rs`, the `cli.caption.download.*` corpus cases |
| E2 | A login during stdio MCP prints only to stderr and never reads stdin | R3 lane 10, `crates/youtube-auth` flow tests |
| E3 | Every token refresh is persisted atomically | `crates/youtube-auth/tests/flow.rs` |
| E4 | Tokeninfo by POST, cached until expiry, optional client allowlist | `crates/youtube-mcp/tests/http.rs`, R8 lanes 9 and 10 |
| E5 | stdio MCP traffic logged only at DEBUG | R7 lane 8 |
| E8 | HTTP MCP timeouts and Origin checks | `crates/youtube-mcp/tests/http.rs`, R8 lanes 5 and 7 |
| E9 | The agent cannot approve its own writes | `crates/youtube-agent/tests/confirm.rs`, `crates/youtube-agent/tests/web_confirm.rs`, R9 lanes 5 and 6, R10 lane 9 |
| E11 | Per-launch tokens on every agent web route | `crates/youtube-agent/tests/auth.rs` |
| E12 | `--otel_to_cloud` accepted and ignored | R10 lane 8 |
| E13 | The skill lists only real tools | `xtask/tests/skillgen.rs` |
| E15 | `install.sh` verifies checksums and attestations | `xtask/tests/install.rs` |

E6, E7, E10 and E14 are correctness and usability fixes with no security effect.

## Verification at R12

**Supply chain.** `cargo audit` checked 202 crates against 1,338 advisories and found nothing. `cargo deny check` passed advisories, bans, licenses and sources. Its only warnings say two allowed licenses (MIT-0, BSD-2-Clause) match no dependency. Output is in `docs/evidence/r12/r12-l4-audit.txt`.

**Fuzzing.** Two libFuzzer targets ran for 30 minutes each. One covers token, credential, base64 and path parsing in `youtube-auth`, the other decoding hostile API responses in `youtube-api`. They made 22.4 million and 25.3 million executions with no crash, panic or failed assertion. See `docs/evidence/r12/r12-l6-fuzz.txt`.

**Unsafe code.** Every crate has `#![forbid(unsafe_code)]`. The only `unsafe` code is two `std::env::set_var` calls, each in an integration-test binary that holds that single test.

**Independent review.** Two audits during the build (R0 to R2 and R3 to R7) were addressed before R12. The R12 review (`docs/evidence/r12/r12-review-3aae399.md`) made 123 attempts on the CLI and MCP gates and broke neither. It broke the agent's gate twice:
- **C1.** The console accepted an approval from stdin lines read ahead of the question.
- **W1.** The web launchers' bearer token doubled as the browser cookie.

It also reported five Low findings (A1 to A4, W2). All seven are fixed in commit `94e24a1`, each with a test.

A second independent reviewer re-attacked `94e24a1` (`docs/evidence/r12/r12-rereview-94e24a1.md`). It confirmed all seven fixes and passed lanes 10 and 3. It also found four new Lows, fixed in the next commit:
- **N1.** A paste of several thousand lines still sitting in the terminal could answer the console question. The console now discards input the terminal holds before asking. Each line carries the time the kernel delivered it, and only a line delivered after the question can answer it.
- **N2.** The A1 fix refused every caption download when the token or credential was given inline. A value that names no file is now treated as an inline secret, as youtube-auth treats it.
- **N3.** A model could hide the real question behind terminal escape codes in its arguments. Confirmation messages shown by the agent and sent in MCP elicitations now print control characters as visible escapes.
- **N4.** The dev UI cookie reaches every localhost port, so an image in a rendered reply could carry it to another local server. UI pages now send `Content-Security-Policy: img-src 'self' data: blob:`.

The access log also no longer records `token=` values.

Three final independent reviews of `c84e7f4` (security, correctness, operability) followed. The security review found one blocker: with `--provider codex`, Codex still exposed its ChatGPT connectors (Gmail, GitHub, Drive, web search) and passed youtube's whole environment to its shell, so a prompt injection in YouTube data could have reached the token and the operator's other accounts without approval. The fix switches off every Codex feature that gives the model a tool and limits the child's environment to an allowlist. A real Codex run then answered that it could run nothing, where the same request under the previous flags read a local file and counted 43 environment variables. The reviews' other findings are fixed or listed below:
- Console approvals now wait for the terminal to be quiet for 150 ms before asking, and decline if input never stops. A long paste had still answered 2 times in 10 with an instant mock model.
- Confirmation text also escapes bidi and zero-width characters.
- A failed caption download leaves an existing file untouched.
- CLI provider errors show the CLI's own message.
- The CLI is killed with everything it started, and its output is capped while it is read.

A last verification round on `3c46de5` confirmed these fixes with real Claude Code and Codex calls: no canary leaked, no network listener was reached and no write reached the recorder. It also found a way around the console approval. A model's own reply could end with terminal escapes that conceal the question that follows (such as SGR 8, concealed text, or an unterminated OSC string). A final check showed that resetting modes left gaps: setting the terminal's default colours (OSC 10/11), margins (DECSLRM) or character set could still hide or garble the question. So whenever the console's output is a terminal, it no longer passes control characters from model text, or from agent errors that can carry it, at all. It shows them as visible escapes, keeping only newlines and tabs, and still cancels any open sequence and resets attributes before asking. Error lines also escape bidi, zero-width, tag and variation-selector characters. This holds even when stdin is piped and approvals are off. A fresh reviewer then confirmed this on the final head with 26 payloads and real Claude Code and Codex runs, and found the agent error line still printed raw; it now gets the same treatment. Output to a pipe or file keeps the model's text as written, as Go does. The tool-call line, approval text and CLI error messages also escape control, bidi, zero-width, tag and variation-selector characters.

## Accepted risks

- **An MCP client that answers the confirmation is trusted.** A client that sends elicitation `accept`, or MRTR `inputResponses.confirm=accept` up front, runs the write. This matches Go. The server cannot tell a human-backed client from one that auto-accepts, so connect only MCP clients that ask you.
- **The dev web UI loads Google Fonts.** The vendored ADK UI makes the browser fetch from `fonts.googleapis.com` and `fonts.gstatic.com`, as Go's does. This is browser traffic, not binary egress. The bundle is stored brotli-compressed in the binary, so `xtask egress` sees none of its URLs; the vendored source is `crates/youtube-agent/assets/webui`.
- **HTTP MCP accepts tokens from any Google OAuth client unless `YOUTUBE_CLI_MCP_ALLOWED_CLIENT_IDS` is set.** Verified tokens are cached until they expire, so a revoked token keeps working until then. Set the allowlist when serving MCP over HTTP.
- **`install.sh` without `gh` cannot verify the attestation.** It verifies the checksum, says it skipped the attestation, and continues (E15).
- **`install.sh --with-skill` runs `npx --yes skills add` without a pinned version.** It runs only on request.
- **Codex features are switched off by name.** A future Codex release could add a tool under a new feature name, which youtube would not know to disable. Codex also accepts unknown `-c` keys silently. Re-run the E16 real-model check (`parity/agent_lane.py real-cli`) after upgrading Codex.
- **Two agent web UIs on different localhost ports share the sign-in cookie name**, so signing in to one signs the other out. This fails safe.
- **`--yes false` (two words) confirms.** pflag reads `false` as an extra argument. Go behaves the same, and the operator still typed `--yes`.

## Reporting a vulnerability

The repository is not published yet. Once it is, report vulnerabilities through GitHub's private vulnerability reporting on it, not in a public issue.
