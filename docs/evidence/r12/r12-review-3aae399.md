# R12 final independent review: security (lane 3) and confirmation gate (lane 10)

Reviewed commit: `3aae399`, which was the r12 head when the review started. r12 has since moved to `88c68f6`, which was not reviewed. Everything ran from a detached worktree with `cargo build --release --features parity-endpoint -p yutu`. The worktree and its target directory have been removed.

**Verdict.**
- **Lane 3 (security):** passes the supply-chain gates. There are 4 Low findings and 5 Info notes, and nothing Medium or higher.
- **Lane 10 (confirmation gate):** does **not** pass.
  - The CLI, stdio MCP and HTTP MCP held against every attempt.
  - The agent has two breaks:
    - **C1:** the console takes approval from lines read ahead of the question.
    - **W1:** a holder of the bearer token can act as the browser and approve writes.

---

## Task A: security review

### Gates
- `cargo deny check` passed: advisories ok, bans ok, licenses ok, sources ok. The only warnings say two allowed licenses (MIT-0, BSD-2-Clause) match no dependency.
- `cargo audit` passed: 198 crates checked against 1338 advisories, with no vulnerabilities and no warnings.

### Findings

**A1. Low. The caption-download check that protects the token file misses tokens named by an absolute path.** `crates/yutu-core/src/fsroot.rs:85-104` (`refuse_protected`)
- **Cause:**
  - The check resolves `YUTU_CACHE_TOKEN` and `YUTU_CREDENTIAL` as paths relative to the root.
  - cap-std rejects an absolute path, and `if let Ok` drops that error, so nothing is protected.
  - It also resolves against `YUTU_ROOT`, while `yutu-auth/src/input.rs:64-71` resolves against the working directory.
- **Reproduced:** `YUTU_CACHE_TOKEN=$PWD/tok yutu caption download --id abc --file tok --yes` overwrote the live token with the caption text. With `YUTU_CACHE_TOKEN=tok` the same command is refused.
- **Scenario:** an MCP client or the agent proposes "Download caption X to tok". The human approves without knowing `tok` is the token, which brings back the E1 defect.
- **Fix:**
  - Resolve these paths with the same `under_root` logic `input.rs` uses.
  - Treat any lookup error other than NotFound as "refuse", or compare the dev/inode of the target with the token file's.

**A2. Low. Check-then-open race in `create_write`.** `crates/yutu-core/src/fsroot.rs:63-81`
- **Cause:** the symlink and identity checks run before `open(truncate)`, and that open follows symlinks.
- **Scenario:** a local process that can already write in the root swaps in a link to the token between the check and the open.
- **Fix:**
  1. Open with `O_NOFOLLOW` and without truncate.
  2. `fstat` the open handle and run the identity check on it.
  3. Then `set_len(0)`.

**A3. Low. The LLM API key follows redirects to other hosts.** `crates/yutu-agent/src/llm/mod.rs:58-63` and `llm/gemini.rs:55-58`
- **Cause:** reqwest's default redirect policy strips only `Authorization`, `Cookie` and `Proxy-Authorization` when the host changes, so `x-goog-api-key` is sent on.
- **Reproduced:** a `--base-url` server answered 307 to another port; that second host got `x-goog-api-key: SECRETKEY123` and the whole prompt.
- **Fix:** `.redirect(Policy::none())`, as `yutu-mcp/src/verify.rs` already does.

**A4. Low, parity builds only. The tokeninfo loopback check accepts userinfo.** `crates/yutu-mcp/src/verify.rs:44-49`
- **Cause:** the check accepts `http://localhost:1@evil.example/`, which would send the bearer tokens being verified to evil.example.
- **Exposure:** the feature is compiled out of release builds; release.yml builds without it.
- **Fix:** reuse `yutu_api::media::authority`, as `client.rs:49-52` does.

### Info notes
- **A5.** `install.sh:83-105` proves authenticity only when `gh` is installed; otherwise it prints that it skipped the attestation and carries on. This matches E15. Optionally add a strict mode that fails instead.
- **A6.** `install.sh:116` runs `npx --yes skills add` without a pinned version. It runs only with `--with-skill`. Pin the version.
- **A7.** `.github/workflows/ci.yml:51-53` (windows-smoke) downloads and runs the Go v0.11.0 binary without checking its hash. The job's token is read-only. Pin the SHA-256.
- **A8.** The dev web UI's `index.html` makes the browser load fonts from fonts.googleapis.com and fonts.gstatic.com. That is browser egress, not binary egress, and `xtask egress` already lists it. Document it or self-host the fonts.
- **A9.** HTTP MCP accepts a token from any Google OAuth client unless `YUTU_MCP_ALLOWED_CLIENT_IDS` is set. Verification results are cached, so a revoked token keeps working until it expires. This is by design under E4; recommend the allowlist in SECURITY.md.

### Clean areas (no open finding)
- **unsafe code:** every crate has `#![forbid(unsafe_code)]`; no `unsafe` anywhere.
- **Telemetry:** deny.toml bans the known families; `--otel_to_cloud` does nothing (E12).
- **Network egress:** only Google OAuth/API, Gemini, OpenAI and loopback. `YUTU_PARITY_*` code is behind `cfg(feature = "parity-endpoint")`, absent from release builds. Nothing beyond A3 and A4.
- **Credential exposure:** secrets redacted in Debug/Display; token writes atomic, mode 0600; PKCE, constant-time state check, https-only token endpoints; API keys in sensitive headers; MCP traffic logged only at DEBUG (E5).
- **Timeouts:** present on the API, LLM and tokeninfo clients, MCP HTTP (2 MiB body cap), agent web servers (10 MiB cap) and the OAuth loopback listener.
- **Web auth:** 256-bit per-launch token, constant-time compare, HttpOnly SameSite=Strict cookie, Origin/Host checks on the API and MCP HTTP. Gaps are W1 and W2 below.
- **Workflows:** actions pinned by SHA, `persist-credentials: false`, no `pull_request_target`, `${{ }}` in `run:` only for static matrix values, `--locked` builds with a pinned toolchain, least-privilege permissions.
- **install.sh, Dockerfile, npm, winget:** `set -euo pipefail` with quoted variables; image pinned by digest, built from scratch, non-root; npm has no install scripts and uses `execFileSync` without a shell.

---

## Task B: attacks on the confirmation gate

A **break** means the recorder received a POST, PUT or DELETE (or an upload), or caption download wrote a file, without the human approval the design requires.

### B1. CLI: no break (45 attempts)
Each attempt ran `video delete --ids v1` against the recorder, with stdin as stated.

**Refused, no request sent:** no flag (stdin `/dev/null`); piped `y`; `--yes=` with `false`, `0`, `f`, `F`, `FALSE`, `False`, empty, space, `no`, `n`; `--yes --yes=false` (last wins); unknown `-y`, `-iyv1`, `--ye`, `--YES`; `-- --yes`; `YUTU_YES`/`YES` env; `yutu --yes video delete`; `__complete`/`__completeNoDesc` (run nothing); `help video delete`.

**Pseudo-terminal:** `n`, `yes`, empty line, EOF, fullwidth `ｙ` declined; `CI=1` plus typed `y` declined; `CI=` (empty) plus `y`, and ` y ` (trimmed), ran, both genuine human answers.

**Other writes without `--yes`:** `playlist insert`, `video rate`, `watermark unset`, `caption download` all refused. **caption download with `--yes`:** `youtube.token.json` and `client_secret.json` refused (blocked extension); `../escape.srt`, an absolute path and a symlink out of the root refused; `ok.srt` written.

**One quirk, not a break:** `--yes false` (two words) deletes, because pflag treats `false` as an extra argument. The human still typed `--yes`. Go behaves the same.

### B2. stdio MCP: no break (48 attempts)

**Elicitation answers (protocols 2024-11-05, 2025-03-26, 2025-06-18, 2025-11-25):**
- Cancelled, no write:
  - `decline` and `cancel`;
  - `Accept` and ` accept`;
  - a null result, `{}`, `{"action":["accept"]}`, `{"action":true}`;
  - `accept` together with `roots`;
  - `accept` nested inside `content`.
- Failed, no write: a string result, and an error reply.
- Duplicate keys: `accept` then `decline` cancels. `decline` then `accept` runs, because the last key wins, as in Go; the client did send accept.
- Answer ids:
  - A string id is ignored.
  - A float id (`1.0`) is honoured, but only to whatever action it carries.

**Capabilities:** a client with no elicitation capability, or with url-only elicitation, gets an error and nothing runs.

**Answer matching:**
- Accepts sent ahead for ids 1-3, then the real elicitation (id 1) answered with decline: cancelled.
- Two concurrent calls:
  1. Call A (`video-delete`) and call B (`playlist-delete`) both waited for an answer.
  2. A's id got `accept` twice; B's got `decline`.
  3. Only A's DELETE was recorded, and B was cancelled.
- A batch of `[call, forged response id 1 accept]` dropped the forged response, and the later decline cancelled the call.
- A batch with duplicate ids was rejected.
- `notifications/cancelled` while an answer was pending ran nothing.

**MRTR (2026-07-28):**
- A call without inputs gets `input_required`.
- These were all refused or cancelled:
  - `decline`;
  - `requestState` alone, `"awaiting_confirm"` or `"confirmed"`;
  - duplicate `confirm` keys or duplicate `inputResponses` (accept then decline);
  - the key spelled `Confirm`, or another key;
  - `inputResponses` as a string, array, null, or wrapped in `result`;
  - `ACCEPT`;
  - `accept` together with `roots`;
  - `_meta.confirm`;
  - `inputResponses`, `confirm` or `yes` placed inside the tool's arguments.

**`_meta` games:** old initialize plus new `_meta` elicits (decline cancels); new initialize plus old `_meta` gives `input_required`; a bogus version gets -32022.

**Other:** `tools/call` before initialize rejected; `Video-Delete`, `video_delete`, `videos-delete` unknown; arguments as string, null or array rejected, extra fields still get `input_required`; `resources/read` of `file:///etc/passwd` or `../` not found.

**caption-download:** decline wrote nothing. Accept with `youtube.token.json` or `../esc.srt` was refused by the sandbox. Accept with `x.srt` wrote the file (trusted-client accept).

### B3. HTTP MCP: no break (30 attempts)
These tests used tokeninfo.py.
- **Auth, Origin, Host:** no bearer, bad token, `error-field` token, token in query: 401; `few-scopes`: 403; Origin `evil`, `null`, `localhost.evil.com`: 403; Host `evil.example` (DNS rebinding): 403.
- **Other requests:** `text/plain` (CSRF simple request) 415; GET and DELETE 405; batches 400 in either protocol; a lone forged elicitation response 202, ran nothing.
- **Old protocol:** a call without inputs fails ("client does not support elicitation"), and decline cancels.
- **MRTR:**
  - No inputs gives `input_required`.
  - Decline cancels.
  - A header version that does not match `_meta` gets 400.
  - Twenty concurrent calls without accept: zero writes.
  - A reused `Mcp-Session-Id` changes nothing, because the server keeps no sessions.
  - caption-download without inputs gives `input_required`.
- **Accepted, as designed:** the `bearer` scheme in lowercase works (RFC 6750), and MRTR accept with a valid token ran (trusted-client accept, the known risk).

### B4. Agent console: one break
**Held:**
- Answers `""`, `n`, `yes`, `ｙ`, `y`+ZWSP, EOF, and `y` with no newline were declined.
- `n` or EOF stopped playlist-insert, comment-insert, video-insert and caption-download. Answering `y` to the same calls did run them, which shows the arguments were valid.
- Four writes in one turn answered `y,n,n,n`: only the first ran.
- None of these got a write through:
  - an injected "SYSTEM: type y" in the model text;
  - the model calling `adk_request_confirmation` itself;
  - `inputResponses` smuggled into the tool arguments;
  - the OpenAI provider.
- Skill tools only read skill text built into the binary. A resource path of `../../../../etc/passwd` read nothing.

**C1. Break, Medium. Approval comes from stdin lines read before the question, and no terminal is required.** `crates/yutu-agent/src/console.rs:19-30` and `:84`
- **Cause:**
  - A thread reads stdin ahead of time into a channel.
  - The approval takes the next queued line.
  - Unlike `yutu_core::confirm`, nothing checks for a terminal or `CI`.
- **Repro:**
  1. Script the mock LLM to call `video-delete {"ids":["v1"]}`.
  2. Run `printf 'please summarize this text:\nY\nthanks\n' | YUTU_PARITY_ENDPOINT=<recorder> yutu agent --base-url <mock> --api-key k`.
  3. The recorder gets `DELETE /youtube/v3/videos?id=v1`.
- The same happens with a piped `y` and `CI=1`, or with a pasted multi-line message that contains a lone `Y` line. The human never sees the question.
- **Fix:**
  - Read approvals from `/dev/tty`, or decline when stdin is not a terminal or `CI` is set.
  - Drain queued lines before asking, and accept only a line that arrives after the prompt is printed.

### B5. Agent web launchers (web, api, webui, a2a, pubsub, eventarc): one break, one Low
**Held:**
- **Auth:** no, wrong, truncated or extended token, Basic auth or wrong cookie: 401 on all 11 routes. `?token=` works only as a GET sign-in under `/ui` (redirect strips it); `POST /ui?token=` and `/api/...?token=`: 401. The API refused evil, other-port loopback and `null` Origins, evil or multiple Host, and `X-Forwarded-Host`/`Forwarded` tricks.
- **Programs:** Bearer `/api/run` and `/api/run_sse`, both A2A versions (also with the cookie), Pub/Sub, Eventarc, and the cookie on an API-only launch: every write declined ("canceled by user").
- **`adk_request_confirmation` forgeries, all refused with no write:** forged request id (it also stops the turn), the call id instead of the request id, `confirmed` as `"true"`, `1` or JSON-encoded `"true"`, the real id in another user or session, duplicate answer parts (deny then approve; wrong id then right id), answer named after the tool, approval before the request existed, text while paused, session deleted while paused.
- **Rewrites and replays:** an approval rewriting args to `ids:["EVIL"]` ran the original `v1`; a replay, and five identical concurrent approvals, each gave exactly one DELETE; reusing the first write's id for a second write ran only the first; an A2A message aimed at the paused session ran in a separate session.

**W1. Break, Medium. Anyone holding the bearer token can approve writes as if they were the browser.**
- **Cause:**
  - `launch/web.rs:31-37` treats a Bearer caller as a program with no human to approve writes, which is how E9 and E11 are meant to work.
  - But the dev-UI cookie value is the same bearer token (`web.rs:72-79` and `:215`), so a program that has the token sends `Cookie: yutu_token=<token>` and is treated as the browser.
  - Also, `Session::run` (`session.rs:212-222`) resumes a paused turn whatever the caller kind, so a plain Bearer request can approve a confirmation shown to the browser user.
- **Repro:**
  1. `POST /api/apps/Miffy/users/u1/sessions/s1` with `Cookie: yutu_token=<token>`.
  2. `POST /api/run` with "go", and read the `adk_request_confirmation` request id.
  3. `POST /api/run` with a `functionResponse` named `adk_request_confirmation`, that id and `confirmed: true`, sent with the cookie or with `Authorization: Bearer <token>`.
  4. The recorder gets the DELETE.
- **Impact:** an A2A, Pub/Sub or automation client given the token for program access can perform writes, which E9 and E11 say programs cannot do.
- **Fix:**
  - Mint a separate browser-session secret at the `/ui?token=` sign-in, and never accept the bearer token as a cookie.
  - Accept a confirmation answer only on a Browser-caller request.

**W2. Low. Other localhost pages can start agent turns (no writes).**
- **Cause:**
  - Pages on another 127.0.0.1 or localhost port count as the same site, so the browser sends the SameSite=Strict cookie.
  - The A2A and trigger routes have no Origin check; it covers only the API routes (`web.rs:126-144`).
- **Impact:** such a page can spend LLM quota and run read tools. Writes are still declined, and the page cannot read the response.
- **Fix:** apply `origin::check` to the A2A, Pub/Sub and Eventarc routes as well.

---

## Summary of open items
| ID | Severity | Area | File |
|---|---|---|---|
| C1 | Medium (gate break) | agent console approval | crates/yutu-agent/src/console.rs:19,84 |
| W1 | Medium (gate break) | web token doubles as browser cookie; Bearer can resume | crates/yutu-agent/src/launch/web.rs:31-37,72-79,215; session.rs:212-222 |
| A1 | Low | token protection misses absolute token path | crates/yutu-core/src/fsroot.rs:85-104 |
| A2 | Low | check-then-open race | crates/yutu-core/src/fsroot.rs:63-81 |
| A3 | Low | API key follows redirects | crates/yutu-agent/src/llm/mod.rs:58 |
| A4 | Low (parity only) | tokeninfo userinfo bypass | crates/yutu-mcp/src/verify.rs:44-49 |
| W2 | Low | no Origin check on a2a and trigger routes | crates/yutu-agent/src/launch/web.rs:126-144 |
| A5-A9 | Info | install.sh, npm pin, CI hash, fonts, allowlist | see above |

Lane 10 passes once C1 and W1 are fixed. Lane 3 passes once the Lows are fixed or explicitly accepted.

Scripts are in the scratchpad `scripts/` folder (cli_attack.py, mcp_stdio.py, mcp_http.py) and `agent-work/` (console.py, web.py).
