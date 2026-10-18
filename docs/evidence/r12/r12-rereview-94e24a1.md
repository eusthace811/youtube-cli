# R12 re-review of 94e24a1: agent gate fixes (lane 10) and security Lows (lane 3)

Independent reviewer, second pass, run against a parity-endpoint release build of 94e24a1. The reviewer's tool could not write files, so this is its returned report, recorded by the coordinator. Fix status for N1 to N4, I2 and I4 is at the end.

**Verdict:** lane 10 passes, and lane 3 passes with Lows. All seven findings are fixed and no program-only path to a write remains. The review found one narrow console race (N1) and one regression from the A1 fix (N2).

## Verdict per finding

| ID | Verdict | Evidence |
|---|---|---|
| C1 console approval | FIXED (one residual race, N1) | Piped stdin always prints "not a terminal, declined" with 0 writes: the original pasted lone `Y`, a piped `y`, and `CI=1`. Over a pty, a `y` or ` Y ` typed after the prompt deletes (control). Declined over a pty: `n`, `^D`, `y^D`, `CI=1` or `CI=true` with a typed `y`, `go\ny\n` typed together, a pasted message with a lone `Y`, a `y` typed while the model was thinking (10 ms, 0.5 s, 2 s), and a 3000-character line. |
| W1 bearer token as browser | FIXED | The bearer and UI tokens differ. `/ui/?token=<bearer>` gets 401 and `/ui/?token=<ui>` gets 302 plus the cookie. The UI token sent as Bearer, and the bearer sent as a cookie, get 401 on all 11 routes. A Bearer answer to a browser-paused write produced no write, through `/api/run` or `run_sse`, and the same held for a deny, text, a second paused write, and a replay. A crawl of 21 paths, SSE, the A2A extended card, `tasks/get`, error bodies and stdout as a Bearer caller never showed the UI token. The same-origin browser flow still works. |
| A1 absolute token path | FIXED (but causes regression N2) | All 13 path forms were refused and the token stayed intact. `--file ok.srt` was written (control). |
| A2 check-then-open race | FIXED | 300 downloads while another thread flipped `x.srt` between a file and a symlink to `tok`: 182 ok, 93 refused, 25 ENOENT, token intact. |
| A3 API key follows redirects | FIXED | 301, 302, 303, 307 and 308, for google and openai, with streaming none and sse: all 20 runs ended in "Error 30x", and the redirect target got 0 requests. |
| A4 tokeninfo userinfo | FIXED | `cargo test -p yutu-mcp --features parity-endpoint --lib loopback`: 3 passed. The code rejects `@`, `\` and `%` in the authority. |
| W2 no Origin check on a2a/triggers | FIXED | A cookie with Origin `127.0.0.1:9999`, `localhost:9999`, `evil.example`, `null`, `127.0.0.1:<port>.evil.example` or `https://…` got 403 on all 14 routes, with 0 LLM calls. So did `Host: evil.example` and a lying `Sec-Fetch-Site`. Cookie GETs with no Origin still return 200. |

Earlier lanes re-run: `cli_attack.py` gave the same 45 results with no new break, and `mcp_stdio.py` passed all 57 cases. `mcp_http.py` was not re-run: only `verify.rs` changed there, and its unit test passes.

## New findings

Nothing reaches a write without a person at a terminal or the browser.

- **N1, Low.** Lines still in the kernel's terminal input buffer could answer the console prompt. Repro: the mock model answers instantly with `video-delete`, and a single pty write of `go\n` plus 3000 `y\n` lines lands before the prompt. Result: `DELETE /youtube/v3/videos`. With 500 lines, or a 20 ms model delay, nothing was written.
- **N2, Low, a regression from the A1 fix.** An inline (base64 or JSON) `YUTU_CACHE_TOKEN` or `YUTU_CREDENTIAL` made every caption download fail. The identity lookup failed with "name too long", and the check refused on any error.
- **N3, Low, present before this commit.** The model could disguise the console approval question, because control characters in the confirmation message were printed raw. Escape codes in the `video-delete` ids erased the real lines and showed a fake "List video(s)" question. The same raw text went into MCP elicitation messages.
- **N4, Low, theoretical.** The UI cookie reaches every localhost port, and the dev UI renders markdown images with no Content-Security-Policy. An injected image URL could carry the cookie to another local port.

## Info

- **I1.** A Bearer caller can cancel a browser-paused write. This only affects availability and fails safe.
- **I2.** The access log recorded `GET /ui/?token=<ui_token>`.
- **I3.** Typing `y` before the prompt and pressing Enter after it approves. This is acceptable and matches the CLI gate.
- **I4.** `parity/agent_lane.py` `typed()` could type early when model text contains a prompt marker. A last line without a newline hung until the timeout, and a timeout did not kill the child.

`cargo test -p yutu-core` and `cargo test -p yutu-agent` both passed with 0 failures.

## Overall (reviewer)

Lane 10 (gate attack): PASS. Lane 3 (security review): PASS with Lows. The reviewer recommended fixing N1 and N3 before sign-off, fixing N2 or recording it as a known limitation, and recording N4 and I1 to I4.

## Coordinator follow-up: fix status

All fixes are in the commit after 94e24a1, each verified on a rebuilt parity binary with the reviewer's own scripts.

| Item | Fix | Verification |
|---|---|---|
| N1 | Before asking, the console runs `tcflush(TCIFLUSH)`. stdin is read unbuffered, each line is stamped when its read returns, and only a line stamped after the question can answer. | `pty_console.py race3`: 3000, 500, 100 and 20-line instant pastes all gave 0 writes, and every pasted line stayed as chat (3001 turns). Controls (typed `y` deletes, `n` does not) unchanged. |
| N2 | A value that names no file is an inline secret, as in yutu-auth. | `a1.py`: inline base64 token, inline JSON credential and short inline token all write `ok.srt`. All 13 protected path forms are still refused. `crates/yutu-core/tests/fsroot_inline_token_env.rs`. |
| N3 | `yutu_core::confirm::printable` escapes control characters in agent and MCP confirmation messages. | `esc.py`: the escapes print as visible `\u{1b}` text. Unit test in `confirm.rs`. |
| N4 | UI pages send `Content-Security-Policy: img-src 'self' data: blob:`. | `crates/yutu-agent/tests/auth.rs`. |
| I2 | The access log replaces `token=` values with `REDACTED`. | Unit test in `launch/http.rs`. |
| I4 | `typed()` waits until stdout ends with a prompt, terminates the last line, and kills the child on timeout. | R9 lanes all pass. |
| I1, I3 | Accepted: fails safe, or matches the CLI gate. | |
