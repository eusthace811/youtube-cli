# Security

yutu-rs drives a YouTube channel with OAuth tokens that hold the full `youtube`, `youtube.force-ssl` and `youtube.channel-memberships.creator` scopes. Whoever holds the token can edit, delete and upload on the channel. The design goal is that nothing but the operator's explicit action can use it, and that it never leaves the machine except to Google.

## Threat model

| Asset | Threat | Control |
|---|---|---|
| OAuth refresh and access tokens | Leaking through logs, errors, URLs, MCP output or a third-party host | Tokens live in a redacting `Secret` type. The token file is written atomically with mode 0600. Tokeninfo verification POSTs the token (E4). Resumable upload sessions must stay on the upload host or a TLS `*.googleapis.com` host. The parity endpoint override exists only in test builds and only for plain loopback URLs, and CI's `xtask egress --release` fails any release binary that still carries a `YUTU_PARITY_` hook. |
| The channel | Writes the operator did not intend, including through prompt injection in the agent or a malicious MCP client | Every write, destructive and local-file operation needs `--yes`, a `y` typed at a terminal, MCP elicitation acceptance, or a human approval in the agent (E9). The agent cannot approve its own calls. Its console takes approvals only from a terminal, only from a line typed after the question is shown, and never with `CI` set. `caption download` is gated too (E1). |
| Local files | Reading secrets for upload, or overwriting the token or client secret | Every file a user names opens inside the root (`YUTU_ROOT` or the working directory) through cap-std, refusing `..`, absolute paths and escaping symlinks. `.json .env .key .pem .crt .cert .pfx .p12` and well-known secret names are refused for uploads and caption downloads. The active token and credential are also refused by identity (device and inode), however they are named, and the check runs on the opened handle before any truncation (E1). |
| Local network listeners | Other local processes or web pages driving the account | The OAuth callback listens on loopback only for the duration of a login. HTTP MCP verifies Google bearer tokens, rejects non-loopback `Origin` headers and sets timeouts (E8). Agent web launchers print two per-launch tokens: a bearer token for programs and a sign-in token for the dev UI's browser. Neither works in place of the other, only the browser can approve writes, and browser requests get Origin and Host checks on every route (E11). |
| LLM API keys | Leaking to a third party | Keys go only to the configured provider. The LLM clients do not follow redirects, so a redirect cannot carry the key header to another host. |
| The Claude Code and Codex backends (E16) | A prompt-injected model acting through the CLI's own tools instead of yutu's gated ones | yutu runs the CLI only as a model: the conversation goes in on stdin, a reply comes back in a fixed JSON schema, and tool calls return as data that yutu's approval gate handles. Claude Code runs with every built-in tool disabled, no MCP servers, no user settings or hooks and no saved session. Codex runs with a read-only sandbox, web search off, user config and rules ignored, no saved session, in a fresh empty directory that is deleted after each call. Both get their vendors' telemetry opt-outs. |
| The operator's privacy | Telemetry, analytics, update checks | None ship. `deny.toml` bans the known telemetry, analytics, crash-reporting and update-check crates and OpenSSL. `xtask egress` fails the build if the release binary contains a URL for any host outside Google's OAuth and YouTube APIs and the two LLM providers. R12 lane 5 runs the whole corpus with only loopback networking allowed. |
| The supply chain | Compromised or unexpected dependencies, tampered releases | crates.io only. `cargo deny` (advisories, bans, licenses, sources) and `cargo audit` run in CI, builds use `--locked`, and CI actions are pinned by commit SHA. Releases ship checksums, a CycloneDX SBOM and GitHub build-provenance attestations. `install.sh` verifies the checksum and the attestation (E15). |

## Deliberate fixes of Go defects (Appendix E)

`docs/MIGRATING.md` has the user-facing table. `parity/divergences.toml` is the registry the differ enforces, and its `proof` field names each test.

| Id | Fix | Proof |
|---|---|---|
| E1 | `caption download` is gated and writes only through the sandbox, never over a token or secret | `crates/yutu-core/tests/fsroot.rs`, `crates/yutu-core/tests/fsroot_token_env.rs`, the `cli.caption.download.*` corpus cases |
| E2 | A login during stdio MCP prints only to stderr and never reads stdin | R3 lane 10, `crates/yutu-auth` flow tests |
| E3 | Every token refresh is persisted atomically | `crates/yutu-auth/tests/flow.rs` |
| E4 | Tokeninfo by POST, cached until expiry, optional client allowlist | `crates/yutu-mcp/tests/http.rs`, R8 lanes 9 and 10 |
| E5 | stdio MCP traffic logged only at DEBUG | R7 lane 8 |
| E8 | HTTP MCP timeouts and Origin checks | `crates/yutu-mcp/tests/http.rs`, R8 lanes 5 and 7 |
| E9 | The agent cannot approve its own writes | `crates/yutu-agent/tests/confirm.rs`, `crates/yutu-agent/tests/web_confirm.rs`, R9 lanes 5 and 6, R10 lane 9 |
| E11 | Per-launch tokens on every agent web route | `crates/yutu-agent/tests/auth.rs` |
| E12 | `--otel_to_cloud` accepted and ignored | R10 lane 8 |
| E13 | The skill lists only real tools | `xtask/tests/skillgen.rs` |
| E15 | `install.sh` verifies checksums and attestations | `xtask/tests/install.rs` |

E6, E7, E10 and E14 are correctness and usability fixes with no security effect.

## Verification at R12

**Supply chain.** `cargo audit` checked 198 crates against 1,338 advisories and found nothing. `cargo deny check` passed advisories, bans, licenses and sources. Its only warnings say two allowed licenses (MIT-0, BSD-2-Clause) match no dependency. Output is in `docs/evidence/r12/r12-l4-audit.txt`.

**Fuzzing.** Two libFuzzer targets ran for 30 minutes each. One covers token, credential, base64 and path parsing in `yutu-auth`, the other decoding hostile API responses in `yutu-api`. They made 22.4 million and 25.3 million executions with no crash, panic or failed assertion. See `docs/evidence/r12/r12-l6-fuzz.txt`.

**Unsafe code.** Every crate has `#![forbid(unsafe_code)]`. The only `unsafe` is one `std::env::set_var` call in a single-test integration binary.

**Independent review.** Two audits during the build (R0 to R2 and R3 to R7) were addressed before R12. The R12 review (`docs/evidence/r12/r12-review-3aae399.md`) made 123 attempts on the CLI and MCP gates and broke neither. It broke the agent's gate twice:
- **C1.** The console accepted an approval from stdin lines read ahead of the question.
- **W1.** The web launchers' bearer token doubled as the browser cookie.

It also reported five Low findings (A1 to A4, W2). All seven are fixed in commit `94e24a1`, each with a test.

A second independent reviewer re-attacked `94e24a1` (`docs/evidence/r12/r12-rereview-94e24a1.md`). It confirmed all seven fixes and passed lanes 10 and 3. It also found four new Lows, fixed in the next commit:
- **N1.** A paste of several thousand lines still sitting in the terminal could answer the console question. The console now discards input the terminal holds before asking. Each line carries the time the kernel delivered it, and only a line delivered after the question can answer it.
- **N2.** The A1 fix refused every caption download when the token or credential was given inline. A value that names no file is now treated as an inline secret, as yutu-auth treats it.
- **N3.** A model could hide the real question behind terminal escape codes in its arguments. Confirmation messages shown by the agent and sent in MCP elicitations now print control characters as visible escapes.
- **N4.** The dev UI cookie reaches every localhost port, so an image in a rendered reply could carry it to another local server. UI pages now send `Content-Security-Policy: img-src 'self' data: blob:`.

The access log also no longer records `token=` values.

## Accepted risks

- **An MCP client that answers the confirmation is trusted.** A client that sends elicitation `accept`, or MRTR `inputResponses.confirm=accept` up front, runs the write. This matches Go. The server cannot tell a human-backed client from one that auto-accepts, so connect only MCP clients that ask you.
- **The dev web UI loads Google Fonts.** The vendored ADK UI makes the browser fetch from `fonts.googleapis.com` and `fonts.gstatic.com`, as Go's does. This is browser traffic, not binary egress. The bundle is stored brotli-compressed in the binary, so `xtask egress` sees none of its URLs; the vendored source is `crates/yutu-agent/assets/webui`.
- **HTTP MCP accepts tokens from any Google OAuth client unless `YUTU_MCP_ALLOWED_CLIENT_IDS` is set.** Verified tokens are cached until they expire, so a revoked token keeps working until then. Set the allowlist when serving MCP over HTTP.
- **`install.sh` without `gh` cannot verify the attestation.** It verifies the checksum, says it skipped the attestation, and continues (E15).
- **`install.sh --with-skill` runs `npx --yes skills add` without a pinned version.** It runs only on request.
- **CI's windows-smoke job runs the downloaded Go v0.11.0 binary without a pinned hash.** The job's token is read-only and the binary ships nowhere.
- **Codex's read-only sandbox still lets the model read files.** Codex has no switch that removes its shell. A prompt-injected model could read a file the operator can read and put it in its reply. Network access for those commands stays blocked, and the reply can reach YouTube only through a write the human approves. Prefer `--provider claude-code`, whose tools are fully off, when the agent reads untrusted content.
- **`--yes false` (two words) confirms.** pflag reads `false` as an extra argument. Go behaves the same, and the operator still typed `--yes`.

## Reporting a vulnerability

The repository is not published yet. Once it is, report vulnerabilities through GitHub's private vulnerability reporting on it, not in a public issue.
