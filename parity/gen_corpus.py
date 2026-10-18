"""Generates parity cases from the Go command tree and the MCP contract.

Writes parity/corpus/{core,cli,mcp}.json. For every leaf command: a defaults
case, one case per flag, one per enum value, both values of every bool, every
output value, the --yes / no --yes pair for gated commands, and maxResults
0, 1, 20, 21, -1 for lists. For every MCP tool: a defaults call, one call per
property and enum value, and the confirmation round trips for gated tools.
"""
import base64
import json
import re
from pathlib import Path

PARITY = Path(__file__).resolve().parent
DUMP = json.loads((PARITY / "golden" / "go-dump.json").read_text())
CONTRACT = json.loads((PARITY / "golden" / "mcp-contract.json").read_text())
TOOLS = {t["name"]: t for t in CONTRACT["tools"]["result"]["tools"]}

PNG = base64.b64encode(bytes.fromhex("89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d4944415478da63f8ffff3f0005fe02fea7d6a4c40000000049454e44ae426082")).decode()
MP4 = base64.b64encode(bytes.fromhex("000000186674797069736f6d0000020069736f6d69736f32") + b"\0" * 40).decode()
SRT = "1\n00:00:00,000 --> 00:00:01,000\nHello\n"
FILES = {"video.mp4": {"b64": MP4}, "thumb.png": {"b64": PNG}, "caption.srt": SRT, "image.png": {"b64": PNG}}
FILE_FLAGS = {"file", "thumbnail"}
MEDIA = {"video insert", "thumbnail set", "caption insert", "caption update", "caption download",
         "channelBanner insert", "playlistImage insert", "playlistImage update", "watermark set"}
NO_TOOL = {"i18nLanguage list", "i18nRegion list", "videoCategory list"}
DATE_FLAGS = re.compile(r"(published|scheduled|recording|publishAt)", re.I)
ENUM = re.compile(r"^[A-Za-z0-9_]+(\|[A-Za-z0-9_]+)+$")


def leaves(node):
    if not node.get("children"):
        yield node
    for c in node.get("children") or []:
        yield from leaves(c)


def file_for(cmd):
    return {"video insert": "video.mp4", "caption insert": "caption.srt", "caption update": "caption.srt",
            "caption download": "out.srt", "thumbnail set": "thumb.png"}.get(cmd, "image.png")


def sample(cmd, f):
    name, typ = f["name"], f["type"]
    if name in FILE_FLAGS:
        return file_for(cmd) if name == "file" else "thumb.png"
    if typ == "stringSlice":
        return {"ids": "id1,id2", "id": "id1", "parts": "id,snippet", "tags": "t1,t2",
                "abuseTypes": "spam,violence", "types": "video,channel"}.get(name, f"{name}1,{name}2")
    if typ in ("int64", "uint64", "int"):
        return "7"
    if DATE_FLAGS.search(name):
        return "2026-01-02T03:04:05Z"
    return f"val-{name}"


def tool_effect(tool):
    a = tool["annotations"]
    if a.get("readOnlyHint"):
        return "read"
    return "destructive" if a.get("destructiveHint") else "write"


def cli_cases():
    cases = []
    for node in leaves(DUMP["commands"]):
        path = node["path"].split()[1:]
        cmd = " ".join(path)
        if len(path) != 2 or path[0] in ("completion",):
            continue
        flags = [f for f in node["flags"] or [] if f["name"] != "help" and not f["hidden"]]
        names = {f["name"] for f in flags}
        tool = TOOLS.get(f"{path[0]}-{path[1]}")
        effect = "media" if cmd in MEDIA else (tool_effect(tool) if tool else "read")
        gated = "yes" in names
        is_list = any(f["name"] == "output" and f["default"] == "table" for f in flags)
        base = []
        for f in flags:
            if f.get("annotations") and f["name"] != "yes":
                base += [f"--{f['name']}", sample(cmd, f)]
        if gated:
            base.append("--yes")
        files = {k: v for k, v in FILES.items()} if effect == "media" else {}
        slug = "cli." + ".".join(path)

        def add(variant, argv, **extra):
            case = {"id": f"{slug}.{variant}", "group": slug, "effect": effect, "argv": path + argv,
                    "media": effect == "media", "files": files}
            if cmd == "caption download":
                case["collect"] = ["out.srt", "val-file"]
            case.update(extra)
            cases.append(case)

        add("defaults", base)
        if gated:
            add("no-yes", [a for a in base if a != "--yes"])
            add("tty-decline", [a for a in base if a != "--yes"], tty=True, stdin="n\n")
            add("tty-accept", [a for a in base if a != "--yes"], tty=True, stdin="y\n")
        for f in flags:
            n = f["name"]
            if n in ("yes", "output"):
                continue
            if f["type"] == "bool":
                add(f"{n}.true", base + [f"--{n}=true"])
                add(f"{n}.false", base + [f"--{n}=false"])
                continue
            if f["type"] == "string" and ENUM.match(f["usage"]):
                for v in f["usage"].split("|"):
                    add(f"{n}.{v}", strip(base, n) + [f"--{n}", v])
                continue
            if n == "maxResults":
                for v in ("0", "1", "20", "21", "-1"):
                    add(f"maxResults.{v}", strip(base, n) + [f"--{n}", v])
                continue
            add(n, strip(base, n) + [f"--{n}", sample(cmd, f)])
            if f["shorthand"]:
                add(f"{n}.short", strip(base, n) + [f"-{f['shorthand']}", sample(cmd, f)])
        if "output" in names:
            for v in (("json", "yaml", "table", "bogus") if is_list else ("json", "yaml", "silent", "")):
                add(f"output.{v or 'default'}", base + ["--output", v])
        if "maxResults" in names:
            add("paging.fail", strip(base, "maxResults") + ["--maxResults", "0", "-o", "json"], fail={"pageToken": "p2", "status": 500})
        add("api.error", base, fail={"request": 1, "status": 403, "message": "The request is not allowed.", "reason": "forbidden"})
        cases.append({"id": f"{slug}.help", "group": "help", "effect": "core", "argv": path + ["--help"]})
    return cases


def strip(argv, name):
    out, skip = [], False
    for a in argv:
        if skip:
            skip = False
            continue
        if a == f"--{name}":
            skip = True
            continue
        out.append(a)
    return out


def core_cases():
    c = [{"id": "core.version", "group": "version", "effect": "core", "argv": ["version"]},
         {"id": "core.version.hyperlink", "group": "version", "effect": "core", "argv": ["version"], "env": {"FORCE_HYPERLINK": "1"}},
         {"id": "core.version.iterm", "group": "version", "effect": "core", "argv": ["version"], "env": {"TERM_PROGRAM": "iTerm.app", "TERM_PROGRAM_VERSION": "3.5.2"}},
         {"id": "core.version.vte-old", "group": "version", "effect": "core", "argv": ["version"], "env": {"VTE_VERSION": "5000"}},
         {"id": "core.root", "group": "help", "effect": "core", "argv": []},
         {"id": "core.help", "group": "help", "effect": "core", "argv": ["--help"]},
         {"id": "core.help.cmd", "group": "help", "effect": "core", "argv": ["help"]},
         {"id": "core.help.video", "group": "help", "effect": "core", "argv": ["help", "video"]},
         {"id": "core.help.video-list", "group": "help", "effect": "core", "argv": ["help", "video", "list"]},
         {"id": "core.resource", "group": "help", "effect": "core", "argv": ["video"]},
         {"id": "core.unknown", "group": "help", "effect": "core", "argv": ["nope"]},
         {"id": "core.unknown-sub", "group": "help", "effect": "core", "argv": ["video", "nope"]},
         {"id": "core.unknown-flag", "group": "help", "effect": "core", "argv": ["video", "list", "--nope"]},
         {"id": "core.bad-int", "group": "help", "effect": "core", "argv": ["video", "list", "--maxResults", "x"]},
         {"id": "core.missing-required", "group": "help", "effect": "core", "argv": ["video", "delete"]},
         {"id": "core.missing-value", "group": "help", "effect": "core", "argv": ["video", "list", "--ids"]},
         {"id": "core.agent-empty-args", "group": "help", "effect": "core", "argv": ["agent", "--args", ""]},
         {"id": "core.mcp-bad-mode", "group": "help", "effect": "core", "argv": ["mcp", "--mode", "bogus"]},
         {"id": "core.extra-arg", "group": "help", "effect": "core", "argv": ["version", "extra"]}]
    for fmt in ("json", "yaml", "table"):
        c.append({"id": f"core.tty.video-list.{fmt}", "group": "tty", "effect": "read", "tty": True, "argv": ["video", "list", "-o", fmt]})
        c.append({"id": f"core.tty.video-update.{fmt}", "group": "tty", "effect": "write", "tty": True, "argv": ["video", "update", "--id", "id1", "--title", "T", "--yes", "-o", fmt]})
    c.append({"id": "core.tty.ci.video-list.json", "group": "tty", "effect": "read", "tty": True, "env": {"CI": "1"}, "argv": ["video", "list", "-o", "json"]})
    c.append({"id": "core.tty.empty-list.json", "group": "tty", "effect": "read", "tty": True, "argv": ["video", "list", "-o", "json"],
              "fixtures": {"GET_videos": {"items": []}}})
    for fmt in ("json", "yaml", "table"):
        c.append({"id": f"core.empty-list.{fmt}", "group": "empty", "effect": "read", "argv": ["video", "list", "-o", fmt],
                  "fixtures": {"GET_videos": {"items": []}}})
    c.append({"id": "core.delete-yes-missing-ids", "group": "help", "effect": "core", "argv": ["video", "delete", "--yes"]})
    c.append({"id": "core.slice-repeat", "group": "help", "effect": "read", "argv": ["video", "list", "--ids", "a", "--ids", "b,c", "-o", "json"]})
    c.append({"id": "core.slice-quoted", "group": "help", "effect": "read", "argv": ["video", "list", "--ids", '"a,b",c', "-o", "json"]})
    c.append({"id": "core.short-combined", "group": "help", "effect": "read", "argv": ["video", "list", "-ia,b", "-ojson"]})
    c.append({"id": "core.short-eq", "group": "help", "effect": "read", "argv": ["video", "list", "-i=a", "-o=json"]})
    c.append({"id": "core.double-dash", "group": "help", "effect": "read", "argv": ["video", "list", "--", "--ids", "x"]})
    c.append({"id": "core.positional", "group": "help", "effect": "read", "argv": ["video", "list", "extra"]})
    c.append({"id": "core.unknown-short", "group": "help", "effect": "core", "argv": ["video", "list", "-z"]})
    c.append({"id": "core.bool-bad", "group": "help", "effect": "core", "argv": ["playlist", "list", "--mine=maybe"]})
    c.append({"id": "core.uint-bad", "group": "help", "effect": "core", "argv": ["watermark", "set", "--durationMs", "-5"]})
    c.append({"id": "core.suggest", "group": "help", "effect": "core", "argv": ["vidoe"]})
    c.append({"id": "core.help-flag-sub", "group": "help", "effect": "core", "argv": ["video", "list", "-h"]})
    c.append({"id": "core.help-unknown", "group": "help", "effect": "core", "argv": ["help", "nope"]})
    for name, argv, env in (("bad-json", ["auth", "-c", '{"bad":1}'], {}),
                            ("missing-file", ["auth", "-c", "nonexistent.json"], {}),
                            ("env-garbage", ["auth"], {"YUTU_CREDENTIAL": "%%%"}),
                            ("default-missing", ["auth"], {})):
        c.append({"id": f"core.auth.{name}", "group": "auth", "effect": "core", "argv": argv, "env": env})
    for sh in ("bash", "zsh", "fish", "powershell"):
        c.append({"id": f"core.completion.{sh}", "group": "completion", "effect": "core", "argv": ["completion", sh]})
    return c


def init(version):
    caps = {} if version >= "2026-07-28" else {"elicitation": {}}
    return [{"jsonrpc": "2.0", "id": 1, "method": "initialize",
             "params": {"protocolVersion": version, "capabilities": caps, "clientInfo": {"name": "parity", "version": "0"}}},
            {"jsonrpc": "2.0", "method": "notifications/initialized"}]


def call(i, name, args, **extra):
    params = {"name": name, "arguments": args}
    params.update(extra)
    return {"jsonrpc": "2.0", "id": i, "method": "tools/call", "params": params}


def prop_sample(name, schema):
    if "enum" in schema:
        return schema["enum"][0]
    t = schema.get("type")
    if t == "array":
        return {"ids": ["id1", "id2"], "parts": ["id", "snippet"]}.get(name, [f"{name}1", f"{name}2"])
    if t in ("number", "integer"):
        return 7
    if t == "boolean":
        return True
    if name == "file":
        return None
    if DATE_FLAGS.search(name.replace("_", "")):
        return "2026-01-02T03:04:05Z"
    return f"val-{name}"


def mcp_cases():
    cases = []
    for name, tool in sorted(TOOLS.items()):
        res, verb = name.split("-", 1)
        cmd = f"{res} {verb}"
        effect = "media" if cmd in MEDIA else tool_effect(tool)
        props = tool["inputSchema"].get("properties", {})
        required = tool["inputSchema"].get("required", [])
        gated = effect != "read" and name != "caption-download"
        fname = file_for(cmd)
        base = {}
        for r in required:
            base[r] = fname if r == "file" else prop_sample(r, props[r])
        files = dict(FILES) if effect == "media" else {}
        slug = f"mcp.{name}"

        def add(variant, args, version="2025-06-18", elicit="accept", **extra):
            msgs = init(version) + [call(2, name, args)]
            if version >= "2026-07-28" and gated:
                msgs.append(call(3, name, args, inputResponses={"confirm": {"action": elicit}}, requestState="awaiting_confirm"))
            case = {"id": f"{slug}.{variant}", "group": slug, "effect": effect, "surface": "mcp",
                    "media": effect == "media", "files": files, "messages": msgs, "elicit": elicit}
            if name == "caption-download":
                case["collect"] = ["out.srt"]
            case.update(extra)
            cases.append(case)

        add("defaults", base)
        if gated:
            add("decline", base, elicit="decline")
            add("no-elicitation", base, elicit="error")
            add("mrtr.accept", base, version="2026-07-28")
            add("mrtr.decline", base, version="2026-07-28", elicit="decline")
        for p, sch in props.items():
            if p in required and "enum" not in sch:
                continue
            if "enum" in sch:
                for v in sch["enum"]:
                    add(f"{p}.{v}", {**base, p: v})
                continue
            v = fname if p == "file" else prop_sample(p, sch)
            add(p, {**base, p: v})
            if sch.get("type") == "boolean":
                add(f"{p}.false", {**base, p: False})
        if "max_results" in props:
            add("max_results.zero", {**base, "max_results": 0})
        add("bad-type", {**base, "max_results": "x"} if "max_results" in props else {**base, "unknown_prop": 1})
        add("api.error", base, fail={"request": 1, "status": 403, "message": "The request is not allowed.", "reason": "forbidden"})
    for uri in ("i18n://hl", "i18n://language/ja", "i18n://region/fr", "video://category/ja"):
        cases.append({"id": f"mcp.resource.{uri.replace('://', '.').replace('/', '.')}", "group": "mcp.resources",
                      "effect": "read", "surface": "mcp", "messages": init("2025-06-18") + [
                          {"jsonrpc": "2.0", "id": 2, "method": "resources/read", "params": {"uri": uri}}]})
    for v in ("2025-06-18", "2025-11-25", "2026-07-28", "1999-01-01"):
        cases.append({"id": f"mcp.initialize.{v}", "group": "mcp.lists", "effect": "core", "surface": "mcp",
                      "messages": init(v) + [{"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
                                             {"jsonrpc": "2.0", "id": 3, "method": "resources/list", "params": {}},
                                             {"jsonrpc": "2.0", "id": 4, "method": "resources/templates/list", "params": {}},
                                             {"jsonrpc": "2.0", "id": 5, "method": "prompts/list", "params": {}},
                                             {"jsonrpc": "2.0", "id": 6, "method": "ping", "params": {}},
                                             {"jsonrpc": "2.0", "id": 7, "method": "nope", "params": {}},
                                             call(8, "no-such-tool", {})]})
    return cases


# The update commands whose lookup Go never let fetch anything (E18).
E18_UPDATES = tuple(f"cli.{r}.update." for r in ("playlist", "playlistItem", "channel", "comment")) + tuple(
    f"mcp.{r}-update." for r in ("playlist", "playlistItem", "channel", "comment"))


def tag_divergences(cases):
    """Marks the cases a registered Appendix E divergence changes, and adds the
    head-only cases that prove the new behavior against Go's golden."""
    extra = []
    for c in cases:
        cid = c["id"]
        if cid.startswith("cli.caption.download.") or cid.startswith("mcp.caption-download."):
            c["divergences"] = ["E1"]
            if cid.startswith("cli.") and not cid.endswith((".help", ".no-yes", ".tty-decline", ".tty-accept")):
                extra.append({**c, "id": cid + ".yes", "argv": c["argv"] + ["--yes"], "golden_from": cid, "divergences": []})
        if cid.startswith("mcp.") and cid.endswith(".max_results.zero"):
            c["divergences"] = sorted(set(c.get("divergences", [])) | {"E17"})
        if cid.startswith(E18_UPDATES):
            c["divergences"] = sorted(set(c.get("divergences", [])) | {"E18"})
        if cid == "mcp.resource.video.category.ja":
            c["divergences"] = ["E6"]
        if c.get("surface") == "mcp":
            c["divergences"] = sorted(set(c.get("divergences", [])) | {"E5"})
    return cases + extra


def main():
    out = PARITY / "corpus"
    out.mkdir(exist_ok=True)
    total = 0
    for name, cases in (("core", core_cases()), ("cli", cli_cases()), ("mcp", mcp_cases())):
        cases = tag_divergences(cases)
        (out / f"{name}.json").write_text(json.dumps(cases, indent=1, ensure_ascii=False) + "\n")
        total += len(cases)
        print(f"{name}: {len(cases)} cases")
    print(f"total: {total} cases")


if __name__ == "__main__":
    main()
