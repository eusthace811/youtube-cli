"""Starts `yutu agent --args "web ..."` (Go harness or Rust head) against the
mock LLM and the YouTube recorder, waits for its port, and offers a small
HTTP client that carries head's per-launch bearer token (divergence E11).

    with WebAgent(binary, script, "web api", port=18480) as w:
        status, headers, body = w.request("GET", "/api/list-apps")
"""
import http.client
import json
import os
import re
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE / "mockllm"))
from recorder import Recorder  # noqa: E402
from server import Mock  # noqa: E402

TOKEN = re.compile(r"token: ([A-Za-z0-9_-]{20,})")
UI_TOKEN = re.compile(r"sign in the browser with \S+\?token=([A-Za-z0-9_-]{20,})")
KEY = "test-llm-key"


class WebAgent:
    def __init__(self, binary, script, args, port, provider="google", extra=(), overrides=None, env=None):
        self.binary, self.script, self.args, self.port = str(binary), script, args, port
        self.provider, self.extra, self.overrides, self.env = provider, list(extra), overrides, env or {}
        self.token = None
        self.ui_token = None

    def __enter__(self):
        self.rec = Recorder()
        self.rec.reset(overrides=self.overrides)
        self.llm_log = tempfile.NamedTemporaryFile("r", suffix=".jsonl", delete=False)
        self.mock = Mock(self.script, self.llm_log.name)
        base = self.mock.url + ("/v1/" if self.provider == "openai" else "")
        argv = [self.binary, "agent", "-p", self.provider, "--base-url", base, "--api-key", KEY]
        argv += ["-m", "gpt-x"] if self.provider == "openai" else []
        args = self.args
        if args.startswith("web") and "-port" not in args:
            args = args.replace("web", f"web -port {self.port}", 1)
        argv += ["--args", args] + self.extra
        e = {k: v for k, v in os.environ.items() if not k.endswith(("_API_KEY", "_BASE_URL"))}
        e.update(self.env)
        e["YUTU_PARITY_ENDPOINT"] = self.rec.url
        self.err = tempfile.NamedTemporaryFile("w+", suffix=".log", delete=False)
        self.proc = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=self.err, stdin=subprocess.DEVNULL, env=e)
        self.argv = argv
        deadline = time.time() + 20
        while time.time() < deadline:
            if self.proc.poll() is not None:
                raise RuntimeError(f"agent exited {self.proc.returncode}: {self.stderr()}")
            try:
                socket.create_connection(("127.0.0.1", self.port), 0.2).close()
                break
            except OSError:
                time.sleep(0.05)
        else:
            raise RuntimeError("agent did not listen: " + self.stderr())
        time.sleep(0.1)
        log = self.stderr()
        m = TOKEN.search(log)
        self.token = m.group(1) if m else None
        m = UI_TOKEN.search(log)
        self.ui_token = m.group(1) if m else None
        return self

    def stderr(self):
        return Path(self.err.name).read_text()

    def llm(self):
        return [json.loads(line) for line in open(self.llm_log.name)]

    def recorded(self):
        return [f'{r["method"]} {r["path"]} {json.dumps(r["query"], sort_keys=True)}' for r in self.rec.requests]

    def request(self, method, path, body=None, headers=None, auth=True, timeout=60):
        h = dict(headers or {})
        if auth and self.token:
            h.setdefault("Authorization", f"Bearer {self.token}")
        data = None
        if body is not None:
            data = body if isinstance(body, (bytes, str)) else json.dumps(body)
            h.setdefault("Content-Type", "application/json")
        c = http.client.HTTPConnection("127.0.0.1", self.port, timeout=timeout)
        c.request(method, path, body=data, headers=h)
        r = c.getresponse()
        out = r.status, {k.lower(): v for k, v in r.getheaders()}, r.read()
        c.close()
        return out

    def __exit__(self, *exc):
        self.proc.terminate()
        try:
            self.proc.wait(10)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait()
        self.mock.close()
        self.rec.close()
        for f in (self.llm_log.name, self.err.name):
            try:
                os.unlink(f)
            except OSError:
                pass
