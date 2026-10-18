"""Local stand-in for the YouTube Data API that records every request.

Responses come from parity/fixtures, keyed by method and path. List fixtures
hold pages chained by nextPageToken, and the recorder serves the page named by
the pageToken query parameter.
"""
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

FIXTURES = Path(__file__).resolve().parent / "fixtures"


def fixture_key(method, path):
    path = path.lstrip("/")
    for prefix, short in (("upload/youtube/v3/", "upload/"), ("youtube/v3/", "")):
        if path.startswith(prefix):
            path = short + path[len(prefix):]
            break
    if path.startswith("captions/"):
        path = "captions/_"
    return f"{method}_{path.replace('/', '.')}"


def google_error(status, message, reason):
    return {"error": {"code": status, "message": message,
                      "errors": [{"message": message, "domain": "global", "reason": reason}]}}


class Recorder:
    def __init__(self, fixtures=FIXTURES):
        self.fixtures = Path(fixtures)
        self.lock = threading.Lock()
        self.reset()
        handler = type("Handler", (_Handler,), {"recorder": self})
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), handler)
        self.server.daemon_threads = True
        self.url = f"http://127.0.0.1:{self.server.server_address[1]}"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def reset(self, fail=None, overrides=None):
        with self.lock:
            self.requests = []
            self.fail = fail or {}
            self.overrides = overrides or {}

    def close(self):
        self.server.shutdown()

    def load(self, key):
        if key in self.overrides:
            return self.overrides[key]
        f = self.fixtures / f"{key}.json"
        return json.loads(f.read_text()) if f.exists() else None

    def respond(self, method, path, query, body, ctype):
        key = fixture_key(method, path)
        with self.lock:
            self.requests.append({"method": method, "path": path, "query": query,
                                  "ctype": ctype, "body": body.decode("utf-8", "replace")})
            index = len(self.requests)
            fail = self.fail
        token = (query.get("pageToken") or [""])[0]
        if fail and (fail.get("request") == index or (token and fail.get("pageToken") == token)):
            status = fail.get("status", 500)
            return status, "application/json", json.dumps(google_error(status, fail.get("message", "Backend Error"), fail.get("reason", "backendError"))).encode()
        fx = self.load(key)
        if method == "DELETE" or (fx is None and method == "POST" and path.rsplit("/", 1)[-1] in NO_CONTENT):
            return 204, None, b""
        if isinstance(fx, dict) and "raw" in fx:
            return 200, fx.get("ctype", "application/octet-stream"), fx["raw"].encode()
        if isinstance(fx, dict) and "pages" in fx:
            pages = fx["pages"]
            page = next((p for p in pages if p.get("_token", "") == token), pages[0])
            return 200, "application/json", json.dumps({k: v for k, v in page.items() if k != "_token"}).encode()
        if fx is None:
            fx = echo(body, ctype)
        return 200, "application/json", json.dumps(fx).encode()


NO_CONTENT = {"markAsSpam", "setModerationStatus", "rate", "reportAbuse", "unset", "set"}


def echo(body, ctype):
    """A write with no fixture answers with its own JSON body plus a stub id."""
    text = body.decode("utf-8", "replace")
    if ctype.startswith("multipart/"):
        start = text.find("{")
        end = text.find("\r\n--", start)
        text = text[start:end] if start >= 0 else ""
    try:
        data = json.loads(text) if text.strip() else {}
    except ValueError:
        data = {}
    if not isinstance(data, dict):
        data = {}
    data.setdefault("id", "stub-id")
    return data


class _Handler(BaseHTTPRequestHandler):
    """Sends fixed Server and Date headers, which Go echoes in YAML output."""
    recorder = None
    protocol_version = "HTTP/1.1"

    def version_string(self):
        return "parity-recorder"

    def date_time_string(self, timestamp=None):
        return "Sun, 18 Oct 2026 12:00:00 GMT"

    def log_message(self, *args):
        pass

    def _serve(self):
        u = urlsplit(self.path)
        length = int(self.headers.get("Content-Length") or 0)
        if self.headers.get("Transfer-Encoding", "").lower() == "chunked":
            body = self._read_chunked()
        else:
            body = self.rfile.read(length) if length else b""
        status, ctype, payload = self.recorder.respond(
            self.command, u.path, parse_qs(u.query, keep_blank_values=True), body,
            self.headers.get("Content-Type", ""))
        self.send_response(status)
        if ctype:
            self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def _read_chunked(self):
        out = b""
        while True:
            size = int(self.rfile.readline().strip() or b"0", 16)
            if size == 0:
                self.rfile.readline()
                return out
            out += self.rfile.read(size)
            self.rfile.readline()

    do_GET = do_POST = do_PUT = do_DELETE = _serve
