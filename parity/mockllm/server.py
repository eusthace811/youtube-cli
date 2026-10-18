"""Scripted stand-in for the Gemini and OpenAI-compatible chat APIs.

Each model request consumes the next step of a JSON script. A step is either
{"text": "..."} or {"calls": [{"name": "video-list", "args": {...}}]}. Every
request body is appended to the record file so lanes can assert tool traffic.

    python3 -I parity/mockllm/server.py --script s.json --record r.jsonl --port 0
"""
import argparse
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Mock:
    def __init__(self, script, record=None, port=0):
        self.steps, self.record, self.lock, self.i = list(script), record, threading.Lock(), 0
        handler = type("H", (_Handler,), {"mock": self})
        self.server = ThreadingHTTPServer(("127.0.0.1", port), handler)
        self.url = f"http://127.0.0.1:{self.server.server_address[1]}"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def step(self, body):
        with self.lock:
            if self.record:
                with open(self.record, "a") as f:
                    f.write(json.dumps(body) + "\n")
            s = self.steps[self.i] if self.i < len(self.steps) else {"text": "done"}
            self.i += 1
            return s


def gemini(step):
    if "calls" in step:
        parts = [{"functionCall": {"name": c["name"], "args": c.get("args", {})}} for c in step["calls"]]
    else:
        parts = [{"text": step["text"]}]
    return {"candidates": [{"content": {"role": "model", "parts": parts}, "finishReason": "STOP", "index": 0}],
            "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 1, "totalTokenCount": 2},
            "modelVersion": "mock"}


def openai(step, i):
    msg = {"role": "assistant", "content": step.get("text")}
    if "calls" in step:
        msg["tool_calls"] = [{"id": f"call_{i}_{n}", "type": "function",
                              "function": {"name": c["name"], "arguments": json.dumps(c.get("args", {}))}}
                             for n, c in enumerate(step["calls"])]
    return {"id": f"chatcmpl-{i}", "object": "chat.completion", "created": 0, "model": "mock",
            "choices": [{"index": 0, "message": msg, "finish_reason": "tool_calls" if "calls" in step else "stop"}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}}


class _Handler(BaseHTTPRequestHandler):
    mock = None
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("Content-Length") or 0)) or b"{}")
        step = self.mock.step({"path": self.path, "body": body})
        if "chat/completions" in self.path:
            payload = openai(step, self.mock.i)
            if body.get("stream"):
                return self.sse([{**payload, "object": "chat.completion.chunk",
                                  "choices": [{"index": 0, "delta": payload["choices"][0]["message"],
                                               "finish_reason": payload["choices"][0]["finish_reason"]}]}], done=True)
        else:
            payload = gemini(step)
            if "streamGenerateContent" in self.path:
                return self.sse([payload])
        data = json.dumps(payload).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def sse(self, events, done=False):
        data = "".join(f"data: {json.dumps(e)}\r\n\r\n" for e in events) + ("data: [DONE]\r\n\r\n" if done else "")
        data = data.encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--script", required=True)
    ap.add_argument("--record")
    ap.add_argument("--port", type=int, default=0)
    a = ap.parse_args()
    m = Mock(json.load(open(a.script)), a.record, a.port)
    print(m.url, flush=True)
    threading.Event().wait()


if __name__ == "__main__":
    main()
