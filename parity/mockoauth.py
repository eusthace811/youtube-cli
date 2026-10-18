"""A local stand-in for Google's OAuth endpoints, for auth lanes.

Serves `/token` for the authorization_code and refresh_token grants and
records every request, so lanes can check the PKCE verifier, the code and
each refresh without a Google account.
"""
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs


class MockOAuth:
    def __init__(self):
        self.requests = []
        self.refreshes = 0
        self.lock = threading.Lock()
        handler = type("H", (_Handler,), {"mock": self})
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), handler)
        self.url = f"http://127.0.0.1:{self.server.server_address[1]}"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def client_secret(self):
        return json.dumps({"installed": {"client_id": "lane-client.apps.googleusercontent.com", "client_secret": "lane-secret",
                                         "auth_uri": f"{self.url}/auth", "token_uri": f"{self.url}/token",
                                         "redirect_uris": ["http://localhost"]}})

    def grant(self, form):
        with self.lock:
            self.requests.append(form)
            if form.get("grant_type") == ["refresh_token"]:
                self.refreshes += 1
                return {"access_token": f"refreshed-{self.refreshes}", "token_type": "Bearer", "expires_in": 3599}
            code = (form.get("code") or [""])[0]
            return {"access_token": f"issued-for-{code}", "token_type": "Bearer", "refresh_token": "lane-refresh", "expires_in": 3599}

    def close(self):
        self.server.shutdown()


class _Handler(BaseHTTPRequestHandler):
    mock = None

    def log_message(self, *a):
        pass

    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("Content-Length") or 0)).decode()
        payload = json.dumps(self.mock.grant(parse_qs(body))).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)
