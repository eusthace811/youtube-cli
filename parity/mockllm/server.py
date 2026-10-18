"""Scripted stand-in for the Gemini, OpenAI Responses and OpenAI chat APIs.

Each model request consumes the next step of a JSON script. A step is either
{"text": "..."} or {"calls": [{"name": "video-list", "args": {...}}]}. Every
request (path, headers and body) is appended to the record file so lanes can
assert tool traffic and that no YouTube credential reaches the model.

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
        self.server.daemon_threads = True
        self.url = f"http://127.0.0.1:{self.server.server_address[1]}"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def step(self, entry):
        with self.lock:
            if self.record:
                with open(self.record, "a") as f:
                    f.write(json.dumps(entry) + "\n")
            s = self.steps[self.i] if self.i < len(self.steps) else {"text": "done"}
            self.i += 1
            return s, self.i

    def close(self):
        self.server.shutdown()


def gemini(step):
    if "calls" in step:
        parts = [{"functionCall": {"name": c["name"], "args": c.get("args", {})}} for c in step["calls"]]
    else:
        parts = [{"text": step["text"]}]
    return {"candidates": [{"content": {"role": "model", "parts": parts}, "finishReason": "STOP", "index": 0}],
            "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 1, "totalTokenCount": 2},
            "modelVersion": "mock"}


def chat(step, i):
    msg = {"role": "assistant", "content": step.get("text")}
    if "calls" in step:
        msg["tool_calls"] = [{"id": f"call_{i}_{n}", "type": "function",
                              "function": {"name": c["name"], "arguments": json.dumps(c.get("args", {}))}}
                             for n, c in enumerate(step["calls"])]
    return {"id": f"chatcmpl-{i}", "object": "chat.completion", "created": 0, "model": "mock",
            "choices": [{"index": 0, "message": msg, "finish_reason": "tool_calls" if "calls" in step else "stop"}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}}


def responses_output(step, i):
    if "calls" in step:
        return [{"type": "function_call", "id": f"fc_{i}_{n}", "call_id": f"call_{i}_{n}", "name": c["name"],
                 "arguments": json.dumps(c.get("args", {})), "status": "completed"}
                for n, c in enumerate(step["calls"])]
    return [{"type": "message", "id": f"msg_{i}", "role": "assistant", "status": "completed",
             "content": [{"type": "output_text", "text": step["text"], "annotations": []}]}]


def responses(step, i):
    return {"id": f"resp_{i}", "object": "response", "created_at": 0, "model": "mock", "status": "completed",
            "output": responses_output(step, i),
            "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}}


def responses_events(step, i):
    """The Responses API stream: created, one added/delta/done run per output
    item, then completed carrying the whole response."""
    full = responses(step, i)
    events = [{"type": "response.created", "response": {**full, "status": "in_progress", "output": []}}]
    for n, item in enumerate(full["output"]):
        if item["type"] == "message":
            events.append({"type": "response.output_item.added", "output_index": n,
                           "item": {**item, "content": [], "status": "in_progress"}})
            events.append({"type": "response.output_text.delta", "item_id": item["id"], "output_index": n,
                           "content_index": 0, "delta": item["content"][0]["text"]})
            events.append({"type": "response.output_text.done", "item_id": item["id"], "output_index": n,
                           "content_index": 0, "text": item["content"][0]["text"]})
        else:
            events.append({"type": "response.output_item.added", "output_index": n,
                           "item": {**item, "arguments": "", "status": "in_progress"}})
            events.append({"type": "response.function_call_arguments.delta", "item_id": item["id"],
                           "output_index": n, "delta": item["arguments"]})
            events.append({"type": "response.function_call_arguments.done", "item_id": item["id"],
                           "output_index": n, "arguments": item["arguments"]})
        events.append({"type": "response.output_item.done", "output_index": n, "item": item})
    events.append({"type": "response.completed", "response": full})
    return events


class _Handler(BaseHTTPRequestHandler):
    mock = None
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("Content-Length") or 0)) or b"{}")
        step, i = self.mock.step({"path": self.path, "headers": dict(self.headers.items()), "body": body})
        if self.path.split("?")[0].endswith("/responses"):
            if body.get("stream"):
                return self.sse([(e["type"], e) for e in responses_events(step, i)])
            payload = responses(step, i)
        elif "chat/completions" in self.path:
            payload = chat(step, i)
            if body.get("stream"):
                chunk = {**payload, "object": "chat.completion.chunk",
                         "choices": [{"index": 0, "delta": payload["choices"][0]["message"],
                                      "finish_reason": payload["choices"][0]["finish_reason"]}]}
                return self.sse([(None, chunk), (None, "[DONE]")])
        else:
            payload = gemini(step)
            if "streamGenerateContent" in self.path:
                return self.sse([(None, payload)])
        data = json.dumps(payload).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def sse(self, events):
        out = ""
        for name, e in events:
            if name:
                out += f"event: {name}\n"
            out += "data: " + (e if isinstance(e, str) else json.dumps(e)) + "\n\n"
        data = out.encode()
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
