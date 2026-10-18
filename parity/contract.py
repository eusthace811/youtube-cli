"""Dumps or compares the MCP contract and every command's --help text.

    python3 -I parity/contract.py --bin ref/yutu-go            # writes golden
    python3 -I parity/contract.py --bin target/release/yutu --compare parity/golden/mcp-contract.json
"""
import argparse
import json
import subprocess
import sys
from pathlib import Path

PARITY = Path(__file__).resolve().parent
GOLDEN = PARITY / "golden"
LISTS = [("tools", "tools/list"), ("resources", "resources/list"),
         ("templates", "resources/templates/list"), ("prompts", "prompts/list")]


def mcp_contract(binary):
    p = subprocess.Popen([binary, "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, text=True, env={"PATH": "/usr/bin:/bin", "HOME": "/tmp"})

    def call(i, method, params=None):
        p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": i, "method": method, "params": params or {}}) + "\n")
        p.stdin.flush()
        while True:
            m = json.loads(p.stdout.readline())
            if m.get("id") == i:
                return m

    res = {"initialize": call(1, "initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
                                                 "clientInfo": {"name": "contract", "version": "0"}})["result"]}
    p.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
    p.stdin.flush()
    for i, (k, m) in enumerate(LISTS, start=2):
        res[k] = call(i, m)
    p.kill()
    p.wait()
    return res


def command_paths(node, prefix=()):
    path = prefix + ((node["use"].split()[0],) if prefix or node["use"] != "yutu" else ())
    yield path
    for c in node.get("children") or []:
        yield from command_paths(c, path)


def help_texts(binary):
    tree = json.loads((GOLDEN / "go-dump.json").read_text())["commands"]
    out = {}
    env = {"PATH": "/usr/bin:/bin", "HOME": "/tmp"}
    for path in command_paths(tree):
        p = subprocess.run([binary, *path, "--help"], capture_output=True, text=True, env=env)
        out[" ".join(("yutu",) + path)] = {"stdout": p.stdout, "stderr": p.stderr, "exit": p.returncode}
    return out


def diff(a, b, path="$"):
    if type(a) is not type(b):
        return [f"{path}: {json.dumps(a)[:200]} != {json.dumps(b)[:200]}"]
    if isinstance(a, dict):
        out = []
        for k in sorted(set(a) | set(b)):
            if k not in a or k not in b:
                out.append(f"{path}.{k}: only in {'golden' if k in a else 'head'}")
            else:
                out += diff(a[k], b[k], f"{path}.{k}")
        return out
    if isinstance(a, list):
        if len(a) != len(b):
            return [f"{path}: length {len(a)} != {len(b)}"]
        return [d for i, (x, y) in enumerate(zip(a, b)) for d in diff(x, y, f"{path}[{i}]")]
    return [] if a == b else [f"{path}: {json.dumps(a, ensure_ascii=False)[:200]} != {json.dumps(b, ensure_ascii=False)[:200]}"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", required=True)
    ap.add_argument("--compare")
    ap.add_argument("--allow", default="", help="contract paths accepted as Appendix E differences, as id=path-prefix pairs")
    ap.add_argument("--help-only", action="store_true", help="compare only the --help texts against golden")
    args = ap.parse_args()
    if args.help_only:
        found = diff(json.loads((GOLDEN / "help.json").read_text()), help_texts(args.bin), "$help")
        for d in found:
            print(d)
        print(f"help: {len(found)} differences")
        sys.exit(1 if found else 0)
    contract = mcp_contract(args.bin)
    helps = help_texts(args.bin)
    if not args.compare:
        (GOLDEN / "mcp-contract.json").write_text(json.dumps(contract, indent=1, sort_keys=True, ensure_ascii=False) + "\n")
        (GOLDEN / "help.json").write_text(json.dumps(helps, indent=1, sort_keys=True, ensure_ascii=False) + "\n")
        tools = contract["tools"]["result"]["tools"]
        print(f"contract: {len(tools)} tools, {sum(len(t['inputSchema'].get('properties', {})) for t in tools)} properties, {len(helps)} help texts")
        return
    allowed = [tuple(x.split("=", 1)) for x in args.allow.split(",") if x]
    found = diff(json.loads(Path(args.compare).read_text()), contract)
    golden_help = json.loads((GOLDEN / "help.json").read_text())
    found += diff(golden_help, helps, "$help")
    registered, rest = [], []
    for d in found:
        hit = next((i for i, prefix in allowed if d.startswith(prefix)), None)
        (registered if hit else rest).append((hit, d))
    for hit, d in registered:
        print(f"registered {hit}: {d}")
    for _, d in rest:
        print(d)
    print(f"contract: {len(rest)} differences")
    sys.exit(1 if rest else 0)


if __name__ == "__main__":
    main()
