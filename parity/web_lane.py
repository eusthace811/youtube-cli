"""R10 lanes for the web launchers: Go harness vs head on the same mock LLM
and YouTube recorder. Writes docs/evidence/r10.

    python3 -I parity/web_lane.py --go ref/yutu-go-harness --head /tmp/youtube-parity \\
        --shipped /tmp/youtube [--lanes 1,3] [--soak 120]
"""
import argparse
import json
import os
import re
import statistics
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from agent_lane import KEY, SUBSTITUTE, Evidence, calls  # noqa: E402
from web_server import WebAgent  # noqa: E402

ROOT = HERE.parent
OUT = ROOT / "docs/evidence/r10"
TIMESTAMP = re.compile(r"time=\S+ ")
PORT = 18483


def stamp(s):
    return TIMESTAMP.sub("time=T ", s)


def run_body(session, text, streaming=None, user="u1"):
    b = {"appName": "Miffy", "userId": user, "sessionId": session,
         "newMessage": {"role": "user", "parts": [{"text": text}]}}
    if streaming is not None:
        b["streaming"] = streaming
    return b


def sse(raw):
    return [json.loads(c[len("data: "):]) for c in raw.decode().split("\n\n") if c.startswith("data: ")]


def summary(events):
    """(tool calls, function response keys, final text) of a run's events."""
    names, responses, text = [], [], ""
    for e in events:
        if e.get("partial"):
            continue
        for p in (e.get("content") or {}).get("parts", []):
            if "functionCall" in p:
                names.append(p["functionCall"]["name"])
            if "functionResponse" in p:
                responses.append((p["functionResponse"]["name"], sorted(p["functionResponse"]["response"])))
            if p.get("text"):
                text = p["text"]
    return names, responses, text


CHAT = calls(("channel-list", {"mine": True}), ("playlist-list", {"channel_id": "UC1", "max_results": 3})) + [
    {"text": "You have three playlists."}]


def lane1(go, head):
    ev = Evidence(OUT / "r10-l1-api-chat.txt", "lane 1, the same scripted chat through the REST API, Go vs head")
    ev("Each binary runs `agent --args \"web webui api\"` against the mock LLM and the YouTube recorder. The chat")
    ev("goes through the requests the dev UI makes: create a session, then POST /api/run_sse (streaming, as the")
    ev("UI's SSE toggle sends it), then again through POST /api/run. Head's requests carry the bearer token (E11).")
    ok = True
    for path, streaming in (("/api/run_sse", True), ("/api/run", None)):
        res = {}
        for name, binary in (("go", go), ("head", head)):
            with WebAgent(binary, CHAT, "web webui api", PORT) as w:
                w.request("POST", "/api/apps/Miffy/users/u1/sessions/s1", {})
                s, h, b = w.request("POST", path, run_body("s1", "how many playlists do I have?", streaming))
                events = sse(b) if "event-stream" in h.get("content-type", "") else json.loads(b)
                res[name] = (s, summary(events), w.recorded(), len(w.llm()))
        ev()
        ev(f"== {path} streaming={streaming}")
        for name in ("go", "head"):
            s, (names, responses, text), rec, nllm = res[name]
            ev(f"{name:4}: status {s}, tool calls {names}, final reply {text!r}, model requests {nllm}")
            ev(f"      function responses {responses}")
            for r in rec:
                ev(f"      recorder: {r}")
        same = res["go"][0] == res["head"][0] and res["go"][1] == res["head"][1] and res["go"][2] == res["head"][2]
        ev(f"identical status, tool calls, responses, final reply and recorder lines: {same}")
        ok &= same
    return ev.save(ok, "Same tool calls, function responses, recorder lines and final reply on both binaries.")


def sign_in(w):
    """The dev UI's sign-in: GET the token link, keep the cookie (head only)."""
    s, h, _ = w.request("GET", f"/ui/?token={w.ui_token}", auth=False)
    assert s == 302, s
    return h["set-cookie"].split(";")[0]


def lane2(head):
    ev = Evidence(OUT / "r10-l2-ui-read.txt", "lane 2, dev UI load and a read question on head")
    ev(SUBSTITUTE.replace("the real-model lane", "the lane (it needs a real Gemini key)"))
    ev("The browser's requests are replayed in order with the cookie the sign-in link sets.")
    script = calls(("video-list", {"ids": ["v1"]})) + [{"text": "Your video v1 has 42 views."}]
    with WebAgent(head, script, "web webui api", PORT) as w:
        cookie = sign_in(w)
        hdr = {"Cookie": cookie}
        steps = [("GET", "/ui/", None), ("GET", "/ui/assets/config/runtime-config.json", None),
                 ("GET", "/api/list-apps?relative_path=./", None), ("GET", "/api/version", None),
                 ("POST", "/api/apps/Miffy/users/user/sessions", {}),
                 ("GET", "/api/apps/Miffy/users/user/sessions", None)]
        session = None
        for m, p, b in steps:
            s, h, body = w.request(m, p, b, hdr, auth=False)
            ev(f"{m} {p} -> {s} {h.get('content-type')}" + (f" {body[:120]!r}" if len(body) < 400 else f" ({len(body)} bytes)"))
            if p.endswith("/sessions") and m == "POST":
                session = json.loads(body)["id"]
        s, h, body = w.request("POST", "/api/run_sse", run_body(session, "how many views does v1 have?", True, "user"),
                               hdr, auth=False)
        names, responses, text = summary(sse(body))
        ev(f"POST /api/run_sse -> {s}: tool calls {names}, responses {responses}, reply {text!r}")
        ev(f"recorder: {w.recorded()}")
        ok = s == 200 and names == ["video-list"] and responses == [("video-list", ["output"])] and text.startswith("Your video")
    return ev.save(ok, "The UI's load sequence answers 200 and the read question gets the scripted answer from a real video-list call.")


def lane3(go, head):
    ev = Evidence(OUT / "r10-l3-401.txt", "lane 3, /list-apps without the token")
    out = {}
    for name, binary in (("go", go), ("head", head)):
        with WebAgent(binary, [], "web api", PORT) as w:
            out[name] = w.request("GET", "/api/list-apps", auth=False)
            if name == "head":
                with_token = w.request("GET", "/api/list-apps")
    for name, (s, h, b) in out.items():
        ev(f"{name:4}: GET /api/list-apps (no Authorization) -> {s} {h.get('www-authenticate', '')} {b!r}")
    ev(f"head: GET /api/list-apps with the token -> {with_token[0]} {with_token[2]!r}")
    ok = out["head"][0] == 401 and with_token[0] == 200
    return ev.save(ok, "Head answers 401 without the token (Go serves the list to anyone, divergence E11).")


def lane4(head):
    ev = Evidence(OUT / "r10-l4-bind.txt", "lane 4, the listener binds loopback only")
    with WebAgent(head, [], "web webui api a2a pubsub eventarc", PORT) as w:
        lsof = subprocess.run(["lsof", "-nP", f"-iTCP:{PORT}", "-sTCP:LISTEN"], capture_output=True, text=True).stdout
        ev(f"$ lsof -nP -iTCP:{PORT} -sTCP:LISTEN   (pid {w.proc.pid}, default -host)")
        ev(lsof.rstrip())
        lines = [l for l in lsof.splitlines()[1:] if str(w.proc.pid) in l]
        ok = bool(lines) and all("127.0.0.1:" in l for l in lines)
    return ev.save(ok, "Every listening socket of the agent is 127.0.0.1.")


def lane5(go, head):
    ev = Evidence(OUT / "r10-l5-a2a.txt", "lane 5, the A2A agent card, Go vs head")
    cards = {}
    for name, binary in (("go", go), ("head", head)):
        with WebAgent(binary, [], "web a2a", PORT) as w:
            s, h, b = w.request("GET", "/.well-known/agent-card.json")
            cards[name] = (s, h.get("content-type"), b.decode())
    g, h = json.loads(cards["go"][2]), json.loads(cards["head"][2])
    extra = sorted(set(h) - set(g))
    missing = sorted(set(g) - set(h))
    differing = sorted(k for k in set(g) & set(h) if g[k] != h[k])
    ev(f"status/content-type go {cards['go'][:2]} head {cards['head'][:2]}")
    ev(f"keys only in head: {extra}; keys only in Go: {missing}; shared keys that differ: {differing}")
    for k in extra:
        ev(f"  head {k}: {json.dumps(h[k])}")
    stripped = {k: v for k, v in h.items() if k not in ("securitySchemes", "security")}
    text_same = json.dumps(stripped, indent=2, ensure_ascii=False) == cards["go"][2]
    ev(f"head card minus the auth scheme, re-indented, is byte-identical to Go's card text: {text_same}")
    ev("")
    ev("head card:")
    ev(cards["head"][2])
    ok = extra == ["security", "securitySchemes"] and not missing and not differing and text_same
    return ev.save(ok, "The cards are identical apart from head's bearer securitySchemes/security (E11).")


PUBSUB_SAMPLE = {"message": {"attributes": {"key": "value"},
                             "data": "SGVsbG8gQ2xvdWQgUHViL1N1YiEgSGVyZSBpcyBteSBtZXNzYWdlIQ==",
                             "messageId": "2070443601311540", "message_id": "2070443601311540",
                             "publishTime": "2021-02-26T19:13:55.749Z", "publish_time": "2021-02-26T19:13:55.749Z"},
                 "subscription": "projects/myproject/subscriptions/mysubscription"}

CLOUDEVENT = {"specversion": "1.0", "type": "google.cloud.storage.object.v1.finalized",
              "source": "//storage.googleapis.com/projects/_/buckets/my-bucket", "subject": "objects/my-file.txt",
              "id": "1234567890", "time": "2026-10-18T12:00:00.000Z", "datacontenttype": "application/json",
              "data": {"bucket": "my-bucket", "name": "my-file.txt", "contentType": "text/plain", "size": "42"}}


def trigger_lane(go, head, number, name, path, cases):
    ev = Evidence(OUT / f"r10-l{number}-{name}.txt", f"lane {number}, {name} trigger samples, Go vs head")
    ok = True
    for label, body, headers in cases:
        res = {}
        for who, binary in (("go", go), ("head", head)):
            with WebAgent(binary, [{"text": f"{name} handled"}], f"web {name}", PORT) as w:
                s, h, b = w.request("POST", path, body, headers)
                llm = w.llm()
                prompt = llm[0]["body"]["contents"][-1]["parts"][0]["text"] if llm else None
                res[who] = (s, b.decode().strip(), len(llm), prompt)
        ev(f"== {label}")
        for who in ("go", "head"):
            s, b, n, prompt = res[who]
            ev(f"{who:4}: {s} {b}  model requests {n}")
            ev(f"      agent saw: {prompt!r}")
        same = res["go"] == res["head"]
        ev(f"identical status, body and agent input: {same}")
        ok &= same
    return ev.save(ok, "Every sample gets the same status and body from both, and the agent runs on the same input.")


def lane8(head, shipped, xtask_cmd):
    ev = Evidence(OUT / "r10-l8-otel.txt", "lane 8, -otel_to_cloud and xtask egress")
    with WebAgent(head, [], "web -otel_to_cloud api", PORT) as w:
        s, _, _ = w.request("GET", "/api/list-apps")
        err = w.stderr()
    notice = [l for l in err.splitlines() if "telemetry" in l]
    ev(f"web: {notice}  (server still serves: GET /api/list-apps -> {s})")
    p = subprocess.run([str(head), "agent", "--api-key", KEY, "--base-url", "http://127.0.0.1:9/", "--args",
                        "console -otel_to_cloud"], input=b"", capture_output=True, timeout=60)
    cnotice = [l for l in p.stderr.decode().splitlines() if "telemetry" in l]
    ev(f"console: {cnotice}")
    ok = len(notice) == 1 and "telemetry is not supported in youtube-cli" in notice[0] and len(cnotice) == 1
    ev()
    ev(f"$ cargo run -p xtask -- egress {shipped}")
    e = subprocess.run(xtask_cmd + ["egress", str(shipped)], capture_output=True, text=True, cwd=ROOT)
    ev((e.stdout + e.stderr).rstrip())
    ev(f"exit {e.returncode}")
    bundle = sorted((ROOT / "crates/youtube-agent/assets/webui/distr").rglob("*"))
    b = subprocess.run(xtask_cmd + ["egress"] + [str(f) for f in bundle if f.is_file()], capture_output=True,
                       text=True, cwd=ROOT)
    hosts = lambda s: sorted(re.findall(r"unexpected host (\S+):", s))  # noqa: E731
    ev()
    ev("The same check over the vendored dev UI bundle files alone (crates/youtube-agent/assets/webui/distr):")
    ev(f"unexpected hosts: {hosts(b.stdout)}")
    rest = sorted(set(hosts(e.stdout)) - set(hosts(b.stdout)))
    ev(f"hosts in the binary but not in the bundle: {rest}")
    ev("  ai.google.dev: the R9 API-key error text (crates/youtube-agent/src/lib.rs at aaa60ff), printed, never fetched.")
    ev("  docs.rs: rustls's own error texts, present since R9; R11 (e5992d4) lists both as display-only.")
    ev("google.github.io is also the A2A extension URI ADK puts on the card and task metadata, an identifier R10")
    ev("adds; the bundle names the same host in its docs links.")
    ok &= e.returncode == 0
    why = ("The notice prints in both launchers and xtask egress passes." if ok else
           "The notice prints in both launchers, but xtask egress FAILS. The hosts R10 adds are the embedded dev UI "
           "bundle's URLs, served to the browser (the browser fetches fonts.googleapis.com and fonts.gstatic.com when "
           "the UI loads; the rest are doc links and XML namespaces), plus the A2A extension URI. youtube's code fetches "
           "none of them. The remaining two hosts predate R10. Needs a root decision: an xtask category for the served "
           "bundle and the A2A identifier, or a modified bundle.")
    return ev.save(ok, why)


def lane9(head):
    ev = Evidence(OUT / "r10-l9-confirm.txt", "lane 9, a delete in a web UI session waits for approval")
    ev("The dev UI's requests, with its sign-in cookie: the model asks to delete playlist p1, the run pauses at")
    ev("ADK's adk_request_confirmation request, and only the human's confirmation sends the DELETE (E9).")
    script = calls(("playlist-list", {"mine": True}), ("playlist-delete", {"ids": ["p1"]})) + [{"text": "Deleted p1."}]
    with WebAgent(head, script, "web webui api", PORT) as w:
        hdr = {"Cookie": sign_in(w)}
        w.request("POST", "/api/apps/Miffy/users/user/sessions/s1", {}, hdr, auth=False)
        s, _, body = w.request("POST", "/api/run_sse", run_body("s1", "delete my test playlist", True, "user"), hdr,
                               auth=False)
        events = sse(body)
        held = [r for r in w.recorded()]
        request = next(e for e in events if e.get("longRunningToolIds"))
        fc = request["content"]["parts"][0]["functionCall"]
        ev(f"first run_sse -> {s}, {len(events)} events; the last asks:")
        ev(f"  {json.dumps(fc, ensure_ascii=False)}")
        ev(f"  longRunningToolIds {request['longRunningToolIds']}")
        ev(f"recorder while held: {held}")
        time.sleep(1)
        still = w.recorded()
        ev(f"recorder one second later, still held: {still}")
        answer = {"role": "user", "parts": [{"functionResponse": {"id": fc["id"], "name": fc["name"], "response": {
            "confirmed": True, "payload": fc["args"]["originalFunctionCall"]["args"]}}}]}
        body = {"appName": "Miffy", "userId": "user", "sessionId": "s1", "newMessage": answer, "streaming": True,
                "functionCallEventId": request["id"]}
        s, _, b = w.request("POST", "/api/run_sse", body, hdr, auth=False)
        names, responses, text = summary(sse(b))
        after = w.recorded()
        ev(f"confirmation run_sse -> {s}: responses {responses}, reply {text!r}")
        ev(f"recorder after approval: {after}")
        ok = (not any(r.startswith("DELETE") for r in still)
              and any(r.startswith("DELETE /youtube/v3/playlists") for r in after) and text == "Deleted p1.")
    return ev.save(ok, "No DELETE reached YouTube until the human confirmed; then exactly the approved one did.")


def sample(pid):
    out = subprocess.run(["ps", "-o", "rss=,time=", "-p", str(pid)], capture_output=True, text=True).stdout.split()
    rss = int(out[0]) * 1024
    t = out[1]
    parts = [float(x) for x in t.replace("-", ":").split(":")]
    cpu = parts[-1] + (parts[-2] * 60 if len(parts) > 1 else 0) + (parts[-3] * 3600 if len(parts) > 2 else 0)
    return rss, cpu


def lane10(head, seconds):
    ev = Evidence(OUT / "r10-l10-idle.txt", f"lane 10, idle soak of `web webui api`, {seconds} s")
    if seconds < 600:
        ev(f"SHORTENED: the plan asks for 10 minutes; this run soaks {seconds} s, as the brief allows.")
    with WebAgent(head, [], "web webui api", PORT) as w:
        time.sleep(2)
        rows = []
        start = time.time()
        while True:
            rss, cpu = sample(w.proc.pid)
            rows.append((time.time() - start, rss, cpu))
            if time.time() - start >= seconds:
                break
            time.sleep(10)
    for t, rss, cpu in rows:
        ev(f"t={t:6.1f}s  RSS {rss / 2**20:6.2f} MiB  CPU time {cpu:.2f}s")
    wall = rows[-1][0] - rows[0][0]
    cpu_pct = (rows[-1][2] - rows[0][2]) / wall * 100
    rss_vals = [r for _, r, _ in rows]
    drift = (max(rss_vals) - min(rss_vals)) / 2**20
    ev(f"CPU over the soak: {cpu_pct:.3f}%  RSS range {drift:.2f} MiB")
    ok = cpu_pct < 1 and drift < 1
    return ev.save(ok, "CPU under 1% and RSS flat (range under 1 MiB).")


def args_lane(go, head):
    ev = Evidence(OUT / "r10-args.txt", "launcher arguments and errors, Go vs head")
    ev("`agent --args X` with flags each launcher rejects; stdout, stderr (timestamps masked) and exit code.")
    ok = True
    for a in ["web", "web foo", "web api api", "web api -bogus", "web -port x", "web -port 99999 api",
              "web pubsub -trigger_max_retries 0", "web eventarc -trigger_max_delay 0s", "web webui -h",
              "web -h", "web a2a -a2a_agent_url", "web -read-timeout 1x api"]:
        res = {}
        for who, binary in (("go", go), ("head", head)):
            p = subprocess.run([str(binary), "agent", "--api-key", KEY, "--args", a], capture_output=True,
                               timeout=30, stdin=subprocess.DEVNULL)
            res[who] = (p.returncode, p.stdout.decode(), stamp(p.stderr.decode()))
        same = res["go"] == res["head"]
        ev(f"== --args {a!r}: identical {same}")
        if not same:
            for i, part in enumerate(("exit", "stdout", "stderr")):
                if res["go"][i] != res["head"][i]:
                    ev(f"  {part} go:   {res['go'][i]!r}"[:1500])
                    ev(f"  {part} head: {res['head'][i]!r}"[:1500])
            ok &= a == "web -port 99999 api"
    ev("Expected difference: `web -port 99999 api` gets as far as the startup lines, where head adds its token")
    ev("line (E11) before the listen error.")
    return ev.save(ok, "Every other launch error is byte-identical; the one difference is head's token line.")


def perf(go, head, runs=3, n=30):
    ev = Evidence(OUT / "r10-perf.txt", f"perf, `web webui api` idle RSS and /run round-trip p50 ({n} calls)")
    ev("Alternating launches of the Go harness and head. Idle RSS: `ps -o rss` 2 s after the port opens.")
    ev(f"Latency: {n} sequential POST /api/run calls, each one scripted text reply from the in-process mock LLM,")
    ev("timed at the client (fresh connection each); p50 per launch.")
    rows = {"go": [], "head": []}
    for i in range(runs):
        for name, binary in (("go", go), ("head", head)):
            with WebAgent(binary, [{"text": f"reply {k}"} for k in range(n + 1)], "web webui api", PORT) as w:
                time.sleep(2)
                rss, _ = sample(w.proc.pid)
                w.request("POST", "/api/apps/Miffy/users/u1/sessions/s1", {})
                w.request("POST", "/api/run", run_body("s1", "warm up"))
                lat = []
                for k in range(n):
                    t0 = time.perf_counter()
                    s, _, _ = w.request("POST", "/api/run", run_body("s1", f"q{k}"))
                    lat.append(time.perf_counter() - t0)
                    assert s == 200, s
                p50 = statistics.median(lat)
                rows[name].append((rss, p50))
                ev(f"run {i + 1} {name:4}: idle RSS {rss / 2**20:6.2f} MiB  /run p50 {p50 * 1000:6.2f} ms")
    g_rss = statistics.median(r for r, _ in rows["go"])
    h_rss = statistics.median(r for r, _ in rows["head"])
    g_p50 = statistics.median(p for _, p in rows["go"])
    h_p50 = statistics.median(p for _, p in rows["head"])
    ev()
    ev(f"median idle RSS: go {g_rss / 2**20:.2f} MiB head {h_rss / 2**20:.2f} MiB (head/go {h_rss / g_rss:.2f})")
    ev(f"median /run p50: go {g_p50 * 1000:.2f} ms head {h_p50 * 1000:.2f} ms (head/go {h_p50 / g_p50:.2f})")
    ok = h_rss <= 0.5 * g_rss and h_p50 <= g_p50
    return ev.save(ok, "Rule: head idle RSS at most 0.5x Go and head p50 at or below Go.")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--go", required=True)
    ap.add_argument("--head", required=True, help="head built with the parity-endpoint feature")
    ap.add_argument("--shipped", help="head as shipped, for xtask egress")
    ap.add_argument("--lanes", default="1,2,3,4,5,6,7,8,9,10,args,perf")
    ap.add_argument("--soak", type=int, default=120)
    a = ap.parse_args()
    go, head = Path(a.go).resolve(), Path(a.head).resolve()
    want = a.lanes.split(",")
    xtask = ["cargo", "run", "-q", "-p", "xtask", "--"]
    results = {}
    if "1" in want:
        results["1"] = lane1(go, head)
    if "2" in want:
        results["2"] = lane2(head)
    if "3" in want:
        results["3"] = lane3(go, head)
    if "4" in want:
        results["4"] = lane4(head)
    if "5" in want:
        results["5"] = lane5(go, head)
    if "6" in want:
        results["6"] = trigger_lane(go, head, 6, "pubsub", "/api/apps/Miffy/trigger/pubsub", [
            ("Pub/Sub push sample", PUBSUB_SAMPLE, None),
            ("no data or attributes", {"message": {"messageId": "1"}, "subscription": "s"}, None),
            ("bad base64", {"message": {"data": "%%%"}}, None)])
    if "7" in want:
        results["7"] = trigger_lane(go, head, 7, "eventarc", "/api/apps/Miffy/trigger/eventarc", [
            ("structured CloudEvent (storage finalized)", CLOUDEVENT, {"Content-Type": "application/cloudevents+json"}),
            ("binary CloudEvent", CLOUDEVENT["data"], {"ce-id": "1234567890", "ce-specversion": "1.0",
                                                       "ce-type": "google.cloud.storage.object.v1.finalized",
                                                       "ce-source": CLOUDEVENT["source"]}),
            ("Pub/Sub message through Eventarc", {**CLOUDEVENT, "type": "google.cloud.pubsub.topic.v1.messagePublished",
                                                  "data": PUBSUB_SAMPLE}, {"Content-Type": "application/cloudevents+json"})])
    if "8" in want:
        results["8"] = lane8(head, Path(a.shipped or a.head).resolve(), xtask)
    if "9" in want:
        results["9"] = lane9(head)
    if "10" in want:
        results["10"] = lane10(head, a.soak)
    if "args" in want:
        results["args"] = args_lane(go, head)
    if "perf" in want:
        results["perf"] = perf(go, head)
    print(results)


if __name__ == "__main__":
    os.environ.setdefault("YOUTUBE_CLI_LOG_LEVEL", "INFO")
    main()
