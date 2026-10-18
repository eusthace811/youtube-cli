"""Generates parity/corpus/mcp-http.json from the MCP corpus.

Each case is a list of HTTP exchanges against `youtube mcp --mode http`, with
tokens the mock tokeninfo (parity/tokeninfo.py) knows. It covers every
tool's defaults call, the lists and resources, a few gated writes over both
protocols, and the transport's auth, routing and error paths.

    python3 -I parity/gen_http_corpus.py
"""
import json
from pathlib import Path

PARITY = Path(__file__).resolve().parent
MCP = json.loads((PARITY / "corpus" / "mcp.json").read_text())
LEGACY = "2025-06-18"
NEW = "2026-07-28"
ACCEPT = "application/json, text/event-stream"
META = {"io.modelcontextprotocol/protocolVersion": NEW, "io.modelcontextprotocol/clientCapabilities": {}}
INIT = {"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": LEGACY, "capabilities": {}, "clientInfo": {"name": "parity", "version": "0"}}}
MRTR_WRITES = ("video-delete", "playlist-insert", "comment-insert")


def post(body, token="good-token", version=LEGACY, headers=None):
    h = {"Content-Type": "application/json", "Accept": ACCEPT}
    if token:
        h["Authorization"] = f"Bearer {token}"
    if version:
        h["Mcp-Protocol-Version"] = version
    h.update(headers or {})
    return {"method": "POST", "path": "/mcp", "headers": h, "body": body}


def padded(size):
    """A ping padded to exactly `size` bytes; the runner builds the body."""
    return {**post(None), "pad": size}


def new_post(method, params, rid=2, name=None, meta=META, headers=None):
    p = {**params, "_meta": meta}
    h = {"Mcp-Method": method}
    if name is not None:
        h["Mcp-Name"] = name
    h.update(headers or {})
    return post({"jsonrpc": "2.0", "id": rid, "method": method, "params": p}, version=NEW, headers=h)


def rpc(rid, method, params=None):
    m = {"jsonrpc": "2.0", "id": rid, "method": method}
    if params is not None:
        m["params"] = params
    return m


def case(cid, exchanges, effect="core", files=None, divergences=()):
    return {"id": f"mcp-http.{cid}", "group": "mcp-http", "effect": effect, "surface": "mcp-http",
            "media": bool(files), "files": files or {}, "exchanges": exchanges,
            "divergences": sorted({"E4", *divergences})}


def tool_cases():
    out = []
    for c in MCP:
        if not c["id"].endswith(".defaults") or not c["id"].startswith("mcp.") or c["group"] == "mcp.lists":
            continue
        call = c["messages"][-1]
        name = call["params"]["name"]
        exchanges = [post(call)]
        if name in MRTR_WRITES:
            params = call["params"]
            exchanges += [
                new_post("tools/call", params, name=name),
                new_post("tools/call", {**params, "inputResponses": {"confirm": {"action": "accept"}},
                                        "requestState": "awaiting_confirm"}, rid=3, name=name),
                new_post("tools/call", {**params, "inputResponses": {"confirm": {"action": "decline"}},
                                        "requestState": "awaiting_confirm"}, rid=4, name=name),
            ]
        out.append(case(f"tool.{name}", exchanges, c["effect"], c.get("files"),
                        [d for d in c.get("divergences", []) if d != "E5"]))
    return out


def resource_cases():
    out = []
    for c in MCP:
        if c["group"] == "mcp.resources":
            msg = c["messages"][-1]
            uri = msg["params"]["uri"]
            out.append(case(f"resource.{uri.replace('://', '.').replace('/', '.')}",
                            [post(msg), new_post("resources/read", {"uri": uri}, name=uri)], "read",
                            divergences=c.get("divergences", []) and [d for d in c["divergences"] if d == "E6"]))
    return out


def protocol_cases():
    lists = [rpc(2, "tools/list", {}), rpc(3, "resources/list", {}), rpc(4, "resources/templates/list", {}),
             rpc(5, "prompts/list", {}), rpc(6, "ping", {}), rpc(8, "tools/call", {"name": "no-such-tool", "arguments": {}})]
    return [
        case("initialize", [post(INIT), post({**INIT, "params": {**INIT["params"], "protocolVersion": "1999-01-01"}}, version=None)]),
        case("lists", [post(m) for m in lists]),
        case("batch", [
            post([rpc(2, "ping"), rpc(3, "ping")], version=None),
            post([rpc(2, "ping"), {"jsonrpc": "2.0", "method": "notifications/initialized"}], version="2025-03-26"),
            post([INIT, {**INIT, "id": 2}], version=None),
            post([rpc(2, "ping")]),
            post([], version=None),
        ]),
        case("no-call", [
            post({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            post({"jsonrpc": "2.0", "id": 9, "result": {}}),
        ]),
        case("invalid", [
            post(rpc(7, "nope", {})),
            post({"jsonrpc": "2.0", "id": None, "method": "ping"}),
            post({"jsonrpc": "1.0", "id": 1, "method": "ping"}),
            post(rpc(1, "notifications/initialized")),
            post(rpc(1, "tools/call")),
            post(rpc(1, "tools/call", None) | {"params": None}),
            post(""),
            post(INIT, headers={"Content-Type": "text/plain"}),
            post(INIT, headers={"Content-Type": "application/json; charset=utf-8"}),
            post(INIT, headers={"Accept": "application/json"}),
            post(INIT, headers={"Accept": "*/*"}),
            post(INIT, version="1999-01-01"),
            post(INIT, headers={"Last-Event-ID": "x_1"}),
            post(INIT, headers={"Host": "evil.example"}),
        ]),
        case("methods", [
            {"method": m, "path": "/mcp", "headers": {"Authorization": "Bearer good-token"}, "body": None}
            for m in ("GET", "DELETE", "PUT", "HEAD")
        ]),
        case("new-protocol", [
            new_post("server/discover", {}),
            new_post("tools/list", {}),
            new_post("ping", {}),
            new_post("initialize", INIT["params"]),
            new_post("nope", {}),
            new_post("tools/list", {}, meta={"io.modelcontextprotocol/protocolVersion": NEW}),
            new_post("tools/list", {}, meta={**META, "io.modelcontextprotocol/clientInfo": "x"}),
            new_post("tools/list", {}, meta={**META, "io.modelcontextprotocol/protocolVersion": "2027-01-01"},
                     headers={"Mcp-Protocol-Version": "2027-01-01"}),
            new_post("tools/list", {}, headers={"Mcp-Method": "ping"}),
            new_post("tools/list", {}, headers={"Mcp-Method": ""}),
            new_post("tools/call", {"name": "video-list", "arguments": {}}),
            new_post("tools/call", {"name": "video-list", "arguments": {}}, name="video-delete"),
            new_post("resources/read", {"uri": "i18n://hl"}, name="i18n://nope"),
            post(rpc(3, "tools/list", {"_meta": META})),
            post(rpc(3, "tools/list", {}), version=NEW, headers={"Mcp-Method": "tools/list"}),
            post({"jsonrpc": "2.0", "method": "notifications/initialized"}, version=NEW,
                 headers={"Mcp-Method": "notifications/initialized"}),
        ]),
        case("cap", [padded(3 << 20), padded(2 << 20), padded((2 << 20) + 1)]),
    ]


def auth_cases():
    tokens = ["nope", "few-scopes", "no-expiry", "error-field", "good-token-2"]
    return [
        case("auth.none", [post(INIT, token=None)]),
        case("auth.tokens", [post(rpc(2, "ping", {}), token=t) for t in tokens]),
        case("auth.header-forms", [
            post(INIT, token=None, headers={"Authorization": a})
            for a in ("bearer good-token", "Bearer  good-token", "Bearer good-token extra", "Basic Z29vZA==", "Bearer")
        ]),
        case("auth.cached", [post(rpc(i, "ping", {})) for i in (2, 3, 4)]),
        case("routes", [
            {"method": "GET", "path": "/.well-known/oauth-protected-resource", "headers": {}, "body": None},
            {"method": "OPTIONS", "path": "/.well-known/oauth-protected-resource", "headers": {}, "body": None},
            {"method": "POST", "path": "/.well-known/oauth-protected-resource", "headers": {}, "body": None},
            {"method": "GET", "path": "/", "headers": {}, "body": None},
            {"method": "GET", "path": "/nope", "headers": {"Authorization": "Bearer good-token"}, "body": None},
            {"method": "POST", "path": "/mcp/", "headers": {}, "body": None},
            {"method": "GET", "path": "/.well-known/oauth-protected-resource/mcp", "headers": {}, "body": None},
        ]),
        case("origin.loopback", [post(INIT, headers={"Origin": o}) for o in
                                 ("http://localhost:3000", "http://127.0.0.1:8216", "http://[::1]")]),
        case("origin.foreign", [post(INIT, headers={"Origin": "https://evil.example"})], divergences=["E8"]),
    ]


def main():
    cases = tool_cases() + resource_cases() + protocol_cases() + auth_cases()
    ids = [c["id"] for c in cases]
    assert len(ids) == len(set(ids))
    (PARITY / "corpus" / "mcp-http.json").write_text(json.dumps(cases, indent=1, ensure_ascii=False) + "\n")
    print(f"mcp-http: {len(cases)} cases")


if __name__ == "__main__":
    main()
