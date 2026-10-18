"""Records the ADK REST, A2A, trigger and web UI surface of `youtube agent
--args "web ..."` and diffs two binaries on it.

`record` runs the corpus below against the Go harness and writes the
normalized exchanges to crates/youtube-agent/tests/fixtures/adk-rest.json, which
crates/youtube-agent/tests/api.rs replays against head. `compare` runs the corpus
on two binaries and prints every exchange that differs.

    python3 -I parity/adk_rest.py record --go ref/yutu-go-harness
    python3 -I parity/adk_rest.py compare --go ref/yutu-go-harness --head /tmp/youtube
"""
import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from web_server import WebAgent  # noqa: E402

FIXTURE = HERE.parent / "crates/youtube-agent/tests/fixtures/adk-rest.json"
ARGS = "web webui api a2a pubsub eventarc"
PORT = 18481

SCRIPT = [
    {"calls": [{"name": "video-list", "args": {"ids": ["v1"]}}]},
    {"text": "Here is the video."},
    {"text": "Streamed reply."},
    {"text": "Plain reply over SSE."},
    {"text": "pubsub handled"},
    {"text": "eventarc handled"},
    {"text": "eventarc binary handled"},
    {"text": "a2a v0 reply"},
    {"text": "a2a v1 reply"},
    {"text": "a2a v0 streamed"},
    {"text": "a2a v1 streamed"},
    {"calls": [{"name": "channel-list", "args": {"ids": ["UC1"]}}]},
    {"text": "a2a tool reply"},
    {"calls": [{"name": "channel-list", "args": {"ids": ["UC2"]}}]},
    {"text": "a2a tool stream reply"},
    {"calls": [{"name": "channel-list", "args": {"ids": ["UC3"]}}]},
    {"text": "a2a v1 tool reply"},
]
BIG = "x" * (10 << 20)

S = "/api/apps/Miffy/users/u1/sessions"
DEV = "/api/dev/apps/Miffy"
MSG = {"role": "user", "parts": [{"text": "list languages"}]}


def run(session, text, streaming=None):
    body = {"appName": "Miffy", "userId": "u1", "sessionId": session,
            "newMessage": {"role": "user", "parts": [{"text": text}]}}
    if streaming is not None:
        body["streaming"] = streaming
    return body


PUBSUB = {"message": {"data": "aGVsbG8gZnJvbSBwdWJzdWI=", "messageId": "1", "publishTime": "2026-01-01T00:00:00Z",
                      "attributes": {"k": "v"}}, "subscription": "projects/p/subscriptions/s"}
CLOUDEVENT = {"specversion": "1.0", "id": "e1", "source": "//storage.googleapis.com/projects/_/buckets/b",
              "type": "google.cloud.storage.object.v1.finalized", "data": {"bucket": "b", "name": "o"}}
CE = {"Content-Type": "application/cloudevents+json"}

# (name, method, path, body, headers). Order matters: the model script is
# consumed in request order.
CORPUS = [
    ("list apps", "GET", "/api/list-apps", None, None),
    ("list apps relative path", "GET", "/api/list-apps?relative_path=./", None, None),
    ("list apps trailing slash", "GET", "/api/list-apps/", None, None),
    ("version", "GET", "/api/version", None, None),
    ("health", "GET", "/health", None, None),
    ("health head", "HEAD", "/health", None, None),
    ("api health", "GET", "/api/health", None, None),
    ("create session with id", "POST", S + "/s1", {"state": {"a": 1}}, None),
    ("create session with id again", "POST", S + "/s1", {}, None),
    ("create session", "POST", S, None, None),
    ("create session bad json", "POST", S + "/s9", "{", None),
    ("get session", "GET", S + "/s1", None, None),
    ("get missing session", "GET", S + "/nope", None, None),
    ("patch session", "PATCH", S + "/s1", {"stateDelta": {"b": "x"}}, None),
    ("patch session scoped key", "PATCH", S + "/s1", {"stateDelta": {"app:b": 1}}, None),
    ("patch missing session", "PATCH", S + "/nope", {"stateDelta": {"b": 1}}, None),
    ("run read tool", "POST", "/api/run", run("s1", "list languages"), None),
    ("run streaming", "POST", "/api/run_sse", run("s1", "stream please", True), None),
    ("run sse not streaming", "POST", "/api/run_sse", run("s1", "plain please"), None),
    ("get session after runs", "GET", S + "/s1", None, None),
    ("run missing session", "POST", "/api/run", run("nope", "x"), None),
    ("run sse missing session", "POST", "/api/run_sse", run("nope", "x"), None),
    ("run unknown field", "POST", "/api/run", {**run("s1", "x"), "bogus": 1}, None),
    ("run missing fields", "POST", "/api/run", {"appName": "Miffy"}, None),
    ("run bad json", "POST", "/api/run", "not json", None),
    ("run unknown app", "POST", "/api/run", {**run("s1", "x"), "appName": "Other"}, None),
    ("run live without upgrade", "GET", "/api/run_live?appName=Miffy&userId=u1&sessionId=s1", None, None),
    ("run live missing params", "GET", "/api/run_live", None, None),
    ("list sessions", "GET", S, None, None),
    ("list artifacts", "GET", S + "/s1/artifacts", None, None),
    ("get artifact", "GET", S + "/s1/artifacts/a.txt", None, None),
    ("get artifact version", "GET", S + "/s1/artifacts/a.txt/versions/0", None, None),
    ("get artifact bad version", "GET", S + "/s1/artifacts/a.txt/versions/x", None, None),
    ("delete artifact", "DELETE", S + "/s1/artifacts/a.txt", None, None),
    ("delete session", "DELETE", S + "/s1", None, None),
    ("delete missing session", "DELETE", S + "/s1", None, None),
    ("build graph", "GET", DEV + "/build_graph", None, None),
    ("build graph image", "GET", DEV + "/build_graph_image", None, None),
    ("builder", "GET", DEV + "/builder", None, None),
    ("builder save", "POST", DEV + "/builder/save", None, None),
    ("builder save tmp", "POST", DEV + "/builder/save?tmp=true", None, None),
    ("builder cancel", "POST", DEV + "/builder/cancel", None, None),
    ("eval sets", "GET", DEV + "/eval_sets", None, None),
    ("create eval set", "POST", DEV + "/eval-sets", {}, None),
    ("create eval set underscore", "POST", DEV + "/eval_sets", {}, None),
    ("get eval set", "GET", DEV + "/eval_sets/e1", None, None),
    ("delete eval set", "DELETE", DEV + "/eval_sets/e1", None, None),
    ("eval cases", "GET", DEV + "/eval_sets/e1/evals", None, None),
    ("get eval case", "GET", DEV + "/eval_sets/e1/evals/c1", None, None),
    ("put eval case", "PUT", DEV + "/eval_sets/e1/evals/c1", {}, None),
    ("delete eval case", "DELETE", DEV + "/eval_sets/e1/evals/c1", None, None),
    ("add session to eval set", "POST", DEV + "/eval_sets/e1/add_session", {}, None),
    ("run eval", "POST", DEV + "/eval_sets/e1/run_eval", {}, None),
    ("eval results", "GET", DEV + "/eval_results", None, None),
    ("eval result", "GET", DEV + "/eval_results/r1", None, None),
    ("metrics info", "GET", DEV + "/metrics-info", None, None),
    ("trace event", "GET", DEV + "/debug/trace/ev1", None, None),
    ("trace session", "GET", DEV + "/debug/trace/session/s1", None, None),
    ("event graph", "GET", DEV + "/users/u1/sessions/s1/events/ev1/graph", None, None),
    ("tests", "GET", DEV + "/tests", None, None),
    ("get test", "GET", DEV + "/tests/t1", None, None),
    ("put test", "PUT", DEV + "/tests/t1", {}, None),
    ("delete test", "DELETE", DEV + "/tests/t1", None, None),
    ("rebuild tests", "POST", DEV + "/tests/rebuild", None, None),
    ("run tests", "POST", DEV + "/tests/run", None, None),
    ("legacy eval sets", "GET", "/api/apps/Miffy/eval_sets", None, None),
    ("legacy eval results", "GET", "/api/apps/Miffy/eval_results", None, None),
    ("legacy create eval set", "POST", "/api/apps/Miffy/eval_sets/x", None, None),
    ("unknown api path", "GET", "/api/nope", None, None),
    ("wrong method", "PUT", "/api/list-apps", None, None),
    ("options known path", "OPTIONS", "/api/list-apps", None, None),
    ("preflight", "OPTIONS", S, None, {"Origin": "http://localhost:8080", "Access-Control-Request-Method": "POST"}),
    ("foreign origin", "GET", "/api/list-apps", None, {"Origin": "http://evil.example"}),
    ("rebinding host", "GET", "/api/list-apps", None, {"Host": "evil.example:8080"}),
    ("pubsub", "POST", "/api/apps/Miffy/trigger/pubsub", PUBSUB, None),
    ("pubsub bad json", "POST", "/api/apps/Miffy/trigger/pubsub", "{", None),
    ("pubsub empty message", "POST", "/api/apps/Miffy/trigger/pubsub", {"message": {}}, None),
    ("pubsub unknown app", "POST", "/api/apps/Other/trigger/pubsub", PUBSUB, None),
    ("pubsub get", "GET", "/api/apps/Miffy/trigger/pubsub", None, None),
    ("eventarc structured", "POST", "/api/apps/Miffy/trigger/eventarc", CLOUDEVENT, CE),
    ("eventarc binary", "POST", "/api/apps/Miffy/trigger/eventarc", {"bucket": "b"},
     {"ce-id": "e2", "ce-type": "google.cloud.storage.object.v1.finalized", "ce-source": "//storage",
      "ce-specversion": "1.0"}),
    ("eventarc pubsub bad data", "POST", "/api/apps/Miffy/trigger/eventarc",
     {**CLOUDEVENT, "type": "google.cloud.pubsub.topic.v1.messagePublished", "data": {"message": {}}}, CE),
    ("eventarc bad json", "POST", "/api/apps/Miffy/trigger/eventarc", "{", CE),
    ("agent card", "GET", "/.well-known/agent-card.json", None, None),
    ("a2a v0 send", "POST", "/a2a/invoke", {"jsonrpc": "2.0", "id": 1, "method": "message/send", "params": {
        "message": {"kind": "message", "messageId": "m1", "role": "user", "parts": [{"kind": "text", "text": "hi"}]}}},
     None),
    ("a2a v1 send", "POST", "/a2a/v1/invoke", {"jsonrpc": "2.0", "id": 2, "method": "SendMessage", "params": {
        "message": {"messageId": "m2", "role": "ROLE_USER", "parts": [{"text": "hi"}]}}}, None),
    ("a2a v0 stream", "POST", "/a2a/invoke", {"jsonrpc": "2.0", "id": 5, "method": "message/stream", "params": {
        "message": {"kind": "message", "messageId": "m3", "role": "user", "parts": [{"kind": "text", "text": "hi"}]}}},
     None),
    ("a2a v1 stream", "POST", "/a2a/v1/invoke", {"jsonrpc": "2.0", "id": 6, "method": "SendStreamingMessage",
                                                  "params": {"message": {"messageId": "m4", "role": "ROLE_USER",
                                                                         "parts": [{"text": "hi"}]}}}, None),
    ("a2a v0 send with tool", "POST", "/a2a/invoke", {"jsonrpc": "2.0", "id": 9, "method": "message/send", "params": {
        "message": {"kind": "message", "messageId": "m5", "role": "user", "parts": [{"kind": "text", "text": "ch"}]}}},
     None),
    ("a2a v0 stream with tool", "POST", "/a2a/invoke", {"jsonrpc": "2.0", "id": 10, "method": "message/stream",
                                                        "params": {"message": {"kind": "message", "messageId": "m6",
                                                                               "role": "user", "parts": [
                                                                                   {"kind": "text", "text": "ch"}]}}},
     None),
    ("a2a v1 send with tool", "POST", "/a2a/v1/invoke", {"jsonrpc": "2.0", "id": 11, "method": "SendMessage",
                                                         "params": {"message": {"messageId": "m7", "role": "ROLE_USER",
                                                                                "parts": [{"text": "ch"}]}}}, None),
    ("a2a cancel missing", "POST", "/a2a/invoke", {"jsonrpc": "2.0", "id": 7, "method": "tasks/cancel",
                                                   "params": {"id": "missing"}}, None),
    ("a2a v1 get task", "POST", "/a2a/v1/invoke", {"jsonrpc": "2.0", "id": 8, "method": "GetTask",
                                                   "params": {"id": "missing"}}, None),
    ("a2a unknown method", "POST", "/a2a/invoke", {"jsonrpc": "2.0", "id": 3, "method": "nope", "params": {}}, None),
    ("a2a bad json", "POST", "/a2a/invoke", "{", None),
    ("a2a get task", "POST", "/a2a/invoke", {"jsonrpc": "2.0", "id": 4, "method": "tasks/get",
                                             "params": {"id": "missing"}}, None),
    ("a2a get", "GET", "/a2a/invoke", None, None),
    ("ui root redirect", "GET", "/", None, None),
    ("ui bare redirect", "GET", "/ui", None, None),
    ("ui index", "GET", "/ui/", None, None),
    ("ui deep link", "GET", "/ui/some/route", None, None),
    ("ui runtime config", "GET", "/ui/assets/config/runtime-config.json", None, None),
    ("ui missing asset", "GET", "/ui/missing.js", None, None),
    ("ui post", "POST", "/ui/", None, None),
    ("not found", "GET", "/nope", None, None),
    ("run body too large", "POST", "/api/run", ('{"appName":"', '"}'), None),
    ("pubsub body too large", "POST", "/api/apps/Miffy/trigger/pubsub", ('{"subscription":"', '"}'), None),
    ("eventarc binary body too large", "POST", "/api/apps/Miffy/trigger/eventarc", ("", ""), {"ce-type": "t"}),
    ("a2a body too large", "POST", "/a2a/invoke", ('{"jsonrpc":"', '"}'), None),
]

UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
KEEP = ("content-type", "allow", "location", "cache-control", "access-control-allow-origin",
        "access-control-allow-methods", "access-control-allow-headers", "vary")
TIME = re.compile(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(\.\d+)?(Z|[+-]\d\d:\d\d)")


def scrub(v):
    if isinstance(v, dict):
        out = {}
        for k, x in v.items():
            if k == "lastUpdateTime" and isinstance(x, (int, float)):
                x = "<time>"
            out[k] = scrub(x)
        return out
    if isinstance(v, list):
        return [scrub(x) for x in v]
    if isinstance(v, str):
        if len(v) > 400:
            return "<sha256:" + hashlib.sha256(v.encode()).hexdigest()[:16] + ">"
        return TIME.sub("<time>", UUID.sub("<uuid>", v))
    return v


def body_of(content_type, raw):
    text = raw.decode("utf-8", "replace")
    if "text/event-stream" in content_type:
        events = []
        for chunk in text.split("\n\n"):
            if not chunk.strip():
                continue
            ev = {}
            for line in chunk.split("\n"):
                k, _, v = line.partition(": ")
                ev[k] = json.loads(v) if k == "data" else v
            events.append(scrub(ev))
        return {"sse": events}
    if "json" in content_type:
        try:
            return {"json": scrub(json.loads(text)), "text": None if text.endswith("\n") else "no-newline"}
        except ValueError:
            pass
    if "text/html" in content_type and len(raw) > 2000:
        return {"sha256": hashlib.sha256(raw).hexdigest()}
    if len(raw) > 2000:
        return {"sha256": hashlib.sha256(raw).hexdigest()}
    return {"text": scrub(text)}


def exchange(w, name, method, path, body, headers):
    """A tuple body is (prefix, suffix) around 10 MiB of 'x', one byte over
    ADK's default cap once wrapped; the fixture stores it as {"big": [...]}."""
    if isinstance(body, tuple):
        sent, stored = body[0] + BIG + body[1] + ("" if body[0] else "x"), {"big": list(body)}
    else:
        sent = body
        stored = body if body is None or isinstance(body, str) else json.dumps(body)
    status, h, raw = w.request(method, path, sent, headers)
    return {
        "name": name,
        "request": {"method": method, "path": path, "headers": headers or {}, "body": stored},
        "status": status,
        "headers": {k: scrub(h[k]) for k in KEEP if k in h},
        "body": body_of(h.get("content-type", ""), raw),
    }


def run_corpus(binary, port=PORT):
    with WebAgent(binary, SCRIPT, ARGS, port) as w:
        out = [exchange(w, *c) for c in CORPUS]
        return out, w.recorded(), w


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["record", "compare"])
    ap.add_argument("--go", required=True)
    ap.add_argument("--head")
    a = ap.parse_args()
    go, go_rec, _ = run_corpus(a.go)
    if a.cmd == "record":
        FIXTURE.parent.mkdir(parents=True, exist_ok=True)
        FIXTURE.write_text(json.dumps({"args": ARGS, "script": SCRIPT, "youtube": go_rec, "exchanges": go},
                                      indent=1, ensure_ascii=False) + "\n")
        print(f"wrote {len(go)} exchanges to {FIXTURE}")
        return
    head, head_rec, _ = run_corpus(a.head)
    same = 0
    for g, h in zip(go, head):
        if g == h:
            same += 1
            if json.dumps(g, ensure_ascii=False) != json.dumps(h, ensure_ascii=False):
                print(f"== {g['name']}: same values, different key order")
                print(f"  go:   {json.dumps(g['body'], ensure_ascii=False)[:600]}")
                print(f"  head: {json.dumps(h['body'], ensure_ascii=False)[:600]}")
            continue
        print(f"== {g['name']}")
        for k in ("status", "headers", "body"):
            if g[k] != h[k]:
                print(f"  {k} go:   {json.dumps(g[k], ensure_ascii=False)[:1500]}")
                print(f"  {k} head: {json.dumps(h[k], ensure_ascii=False)[:1500]}")
    print(f"{same}/{len(go)} exchanges identical; youtube calls identical: {go_rec == head_rec}")


if __name__ == "__main__":
    main()
