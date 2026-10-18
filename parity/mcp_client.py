"""Minimal MCP client for lanes and benchmarks, over stdio or streamable HTTP.

    python3 -I parity/mcp_client.py --stdio target/release/yutu call video-list '{"ids":["a"]}'
    python3 -I parity/mcp_client.py --http http://localhost:8216/mcp --token T list
"""
import argparse
import json
import subprocess
import sys
import urllib.request


class Stdio:
    def __init__(self, binary, version="2025-06-18", env=None, elicit="accept"):
        self.p = subprocess.Popen([binary, "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.DEVNULL, text=True, env=env)
        self.elicit = elicit
        self.next = 0
        self.init = self.request("initialize", {"protocolVersion": version, "capabilities": {"elicitation": {}},
                                                "clientInfo": {"name": "mcp_client", "version": "0"}})
        self.notify("notifications/initialized")

    def notify(self, method, params=None):
        self.p.stdin.write(json.dumps({"jsonrpc": "2.0", "method": method, "params": params or {}}) + "\n")
        self.p.stdin.flush()

    def request(self, method, params=None):
        self.next += 1
        rid = self.next
        self.p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": rid, "method": method, "params": params or {}}) + "\n")
        self.p.stdin.flush()
        while True:
            frame = json.loads(self.p.stdout.readline())
            if "method" in frame and "id" in frame:
                self.p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": frame["id"], "result": {"action": self.elicit}}) + "\n")
                self.p.stdin.flush()
            elif frame.get("id") == rid:
                return frame

    def close(self):
        self.p.stdin.close()
        self.p.wait()


class Http:
    def __init__(self, url, token=None, version="2025-06-18", headers=None):
        self.url, self.token, self.next = url, token, 0
        self.headers = headers or {}
        self.init = self.request("initialize", {"protocolVersion": version, "capabilities": {},
                                                "clientInfo": {"name": "mcp_client", "version": "0"}})

    def request(self, method, params=None):
        self.next += 1
        body = json.dumps({"jsonrpc": "2.0", "id": self.next, "method": method, "params": params or {}}).encode()
        req = urllib.request.Request(self.url, data=body, method="POST", headers={
            "Content-Type": "application/json", "Accept": "application/json, text/event-stream", **self.headers})
        if self.token:
            req.add_header("Authorization", f"Bearer {self.token}")
        with urllib.request.urlopen(req, timeout=30) as r:
            return json.loads(r.read())

    def close(self):
        pass


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--stdio")
    ap.add_argument("--http")
    ap.add_argument("--token")
    ap.add_argument("--version", default="2025-06-18")
    ap.add_argument("--elicit", default="accept")
    ap.add_argument("op", choices=["list", "call", "read"])
    ap.add_argument("name", nargs="?")
    ap.add_argument("args", nargs="?", default="{}")
    a = ap.parse_args()
    c = Stdio(a.stdio, a.version, elicit=a.elicit) if a.stdio else Http(a.http, a.token, a.version)
    if a.op == "list":
        r = c.request("tools/list")
    elif a.op == "read":
        r = c.request("resources/read", {"uri": a.name})
    else:
        r = c.request("tools/call", {"name": a.name, "arguments": json.loads(a.args)})
    json.dump(r, sys.stdout, ensure_ascii=False)
    print()
    c.close()


if __name__ == "__main__":
    main()
