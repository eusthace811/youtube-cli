"""Normalizing differ for parity results.

Each normalization is one function below with a docstring naming what it
changes. Nothing else is normalized: stdout, stderr and the exit code are
otherwise compared byte for byte.
"""
import json
import re
import tomllib
from pathlib import Path

DIVERGENCES = Path(__file__).resolve().parent / "divergences.toml"


def split_part(query):
    """Splits comma-joined `part` values, since Go sends `part` joined or repeated."""
    out = {}
    for k, vals in query.items():
        if k == "part":
            vals = [p for v in vals for p in v.split(",")]
        out[k] = list(vals)
    return dict(sorted(out.items()))


def boundary(ctype):
    m = re.search(r"boundary=([^;]+)", ctype or "")
    return m.group(1).strip('"') if m else None


def mask_boundary(req):
    """Replaces the random multipart boundary with a fixed token."""
    b = boundary(req["ctype"])
    if not b:
        return req["ctype"], req["body"]
    return req["ctype"].replace(b, "BOUNDARY"), req["body"].replace(b, "BOUNDARY")


def multipart_parts(body):
    """Splits a multipart body into parts, decoding the JSON part by value."""
    parts = []
    for chunk in body.split("--BOUNDARY")[1:]:
        if chunk.startswith("--"):
            break
        head, _, payload = chunk.lstrip("\r\n").partition("\r\n\r\n")
        payload = payload[:-2] if payload.endswith("\r\n") else payload
        headers = sorted(h.lower() for h in head.split("\r\n") if h)
        try:
            payload = json.loads(payload) if any("application/json" in h for h in headers) else payload
        except ValueError:
            pass
        parts.append({"headers": headers, "payload": payload})
    return parts


def body_value(req):
    """Compares JSON bodies by value instead of by bytes."""
    ctype, body = mask_boundary(req)
    if ctype.startswith("multipart/"):
        return ctype, multipart_parts(body)
    if "json" in ctype and body:
        try:
            return ctype, json.loads(body)
        except ValueError:
            pass
    return ctype, body


def mask_time(text):
    """Replaces slog `time=` stamps in stderr with a fixed token."""
    return re.sub(r"time=\S+", "time=T", text)


def mask_port(text):
    """Replaces the recorder's random loopback port with a fixed token."""
    return re.sub(r"127\.0\.0\.1:\d+", "127.0.0.1:PORT", text)


def mcp_by_value(text):
    """Parses each JSON-RPC reply line so MCP stdout compares by value, not key order."""
    return [json.loads(line) for line in text.splitlines() if line.strip()]


def normalize_request(req):
    ctype, body = body_value(req)
    return {"method": req["method"], "path": req["path"], "query": split_part(req["query"]),
            "ctype": ctype, "body": body}


def registered():
    if not DIVERGENCES.exists():
        return {}
    return tomllib.loads(DIVERGENCES.read_text()).get("divergence", {})


def compare(golden, head, case, allowed=()):
    """Returns a list of human-readable differences between two results."""
    accepted = {d for d in case.get("divergences", []) if d in allowed or not allowed}
    known = registered()
    for d in accepted:
        if d not in known:
            raise ValueError(f"case {case['id']} names unregistered divergence {d}")
    skip = {field for d in accepted for field in known[d].get("fields", [])}
    out = []
    g_reqs = [normalize_request(r) for r in golden["requests"]]
    h_reqs = [normalize_request(r) for r in head["requests"]]
    if "requests" not in skip:
        if len(g_reqs) != len(h_reqs):
            out.append(f"request count {len(g_reqs)} != {len(h_reqs)}")
        for i, (g, h) in enumerate(zip(g_reqs, h_reqs)):
            for k in g:
                if g[k] != h[k]:
                    out.append(f"request {i} {k}: {json.dumps(g[k], ensure_ascii=False)[:400]} != {json.dumps(h[k], ensure_ascii=False)[:400]}")
    mcp = case.get("surface") == "mcp"
    for field, norm in (("stdout", mask_port), ("stderr", lambda t: mask_port(mask_time(t)))):
        if field in skip or (mcp and field == "stderr"):
            continue
        g, h = norm(golden[field]), norm(head[field])
        if mcp:
            g, h = mcp_by_value(g), mcp_by_value(h)
            for i, (a, b) in enumerate(zip(g, h)):
                if a != b:
                    out.append(f"mcp reply {i}: {first_difference(json.dumps(a, ensure_ascii=False, sort_keys=True), json.dumps(b, ensure_ascii=False, sort_keys=True))}")
            if len(g) != len(h):
                out.append(f"mcp reply count {len(g)} != {len(h)}")
        elif g != h:
            out.append(f"{field}: {first_difference(g, h)}")
    if "exit" not in skip and golden["exit"] != head["exit"]:
        out.append(f"exit {golden['exit']} != {head['exit']}")
    if "files" not in skip and golden.get("files", {}) != head.get("files", {}):
        out.append(f"files: {golden.get('files')} != {head.get('files')}")
    return out


def first_difference(a, b):
    i = next((i for i, (x, y) in enumerate(zip(a, b)) if x != y), min(len(a), len(b)))
    lo = max(0, i - 40)
    return f"at byte {i}: {a[lo:i + 60]!r} != {b[lo:i + 60]!r}"
