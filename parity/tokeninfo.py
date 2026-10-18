"""Local stand-in for Google's tokeninfo endpoint that records every request.

Go's verifier sends GET ?access_token=; head posts a form body (divergence
E4). Both shapes are answered from the same token table.

    python3 -I parity/tokeninfo.py --port 8217
"""
import argparse
import json
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlsplit

SCOPES = [
    "https://www.googleapis.com/auth/youtube",
    "https://www.googleapis.com/auth/youtube.force-ssl",
    "https://www.googleapis.com/auth/youtube.channel-memberships.creator",
]
CLIENT = "client-a.apps.googleusercontent.com"
OTHER = "client-b.apps.googleusercontent.com"


def info(scopes=SCOPES, aud=CLIENT, expires_in="3599", sub="1001"):
    out = {"azp": aud, "aud": aud, "sub": sub, "scope": " ".join(scopes), "exp": "1792324800",
           "access_type": "offline"}
    if expires_in is not None:
        out["expires_in"] = expires_in
    return out


TOKENS = {
    "good-token": (200, info()),
    "good-token-2": (200, info(sub="1002")),
    "few-scopes": (200, info(scopes=SCOPES[:1])),
    "other-aud": (200, info(aud=OTHER)),
    "no-expiry": (200, info(expires_in=None)),
    "error-field": (200, {"error": "invalid_token"}),
}
INVALID = (400, {"error": "invalid_token", "error_description": "Invalid Value"})


class TokenInfo:
    def __init__(self, port=0, delay=0.0):
        self.lock = threading.Lock()
        self.requests = []
        self.delay = delay
        handler = type("Handler", (_Handler,), {"mock": self})
        ThreadingHTTPServer.allow_reuse_address = True
        self.server = ThreadingHTTPServer(("127.0.0.1", port), handler)
        self.server.daemon_threads = True
        self.url = f"http://127.0.0.1:{self.server.server_address[1]}/tokeninfo"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def reset(self):
        with self.lock:
            self.requests = []

    def taken(self):
        with self.lock:
            return list(self.requests)

    def close(self):
        self.server.shutdown()


class _Handler(BaseHTTPRequestHandler):
    mock = None
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def _serve(self):
        u = urlsplit(self.path)
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length).decode("utf-8", "replace") if length else ""
        with self.mock.lock:
            self.mock.requests.append({"method": self.command, "path": u.path, "query": u.query,
                                       "ctype": self.headers.get("Content-Type", ""), "body": body})
        token = (parse_qs(u.query).get("access_token") or parse_qs(body).get("access_token") or [""])[0]
        if self.mock.delay:
            time.sleep(self.mock.delay)
        status, payload = TOKENS.get(token, INVALID)
        data = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=UTF-8")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    do_GET = do_POST = _serve


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=0)
    ap.add_argument("--log", action="store_true", help="print each request as it arrives")
    a = ap.parse_args()
    t = TokenInfo(a.port)
    print(t.url, flush=True)
    seen = 0
    try:
        while True:
            time.sleep(0.2)
            reqs = t.taken()
            if a.log:
                for r in reqs[seen:]:
                    print(json.dumps(r), flush=True)
            seen = len(reqs)
    except KeyboardInterrupt:
        t.close()


if __name__ == "__main__":
    main()
