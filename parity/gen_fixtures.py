"""Writes parity/fixtures/*.json from spec/youtube-v3.json.

Every list endpoint gets three pages chained by nextPageToken. Items fill every
schema field with deterministic values, and one item per page holds zero values
so encoders that drop or keep empty fields are told apart.
"""
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SPEC = json.loads((ROOT / "spec" / "youtube-v3.json").read_text())
SCHEMAS = SPEC["schemas"]
OUT = ROOT / "parity" / "fixtures"
TEXTS = ["Plain title", "Ünïcödé ✓ 日本語のタイトル", "<b>tags</b> & \"quotes\"", "emoji 🎬🎥 mix", "tab\tand\nnewline",
         "true", "123", "key: value", "- dash start", "# hash", " leading space", "null", "~", "@at sign", "'single' quoted",
         "line\u2028sep", "trailing space ", "multi\nline\ntext\n",
         "A very long line that keeps going well past eighty characters so that any wrapping logic in an emitter shows up clearly here"]


def value(schema, path, seed, depth, zero):
    if "$ref" in schema:
        if depth > 4:
            return {}
        return obj(schema["$ref"], path, seed, depth + 1, zero)
    t = schema.get("type")
    fmt = schema.get("format", "")
    if t == "string":
        if fmt in ("int64", "uint64"):
            return "0" if zero else str((seed * 7919 + len(path) * 31) % 1000000)
        if zero:
            return ""
        if fmt == "date-time":
            return f"2026-0{seed % 9 + 1}-1{seed % 9}T12:34:5{seed % 10}Z"
        if "enum" in schema:
            return schema["enum"][seed % len(schema["enum"])]
        return (TEXTS[(seed * 7 + len(path)) % len(TEXTS)] if path.endswith(("title", "name")) else f"{TEXTS[(seed // 10 + seed) % len(TEXTS)]} {path}") if path.endswith(("title", "description", "textDisplay", "name")) else f"{path}-{seed}"
    if t in ("integer",):
        return 0 if zero else (seed * 13 + len(path)) % 5000
    if t == "number":
        return 0 if zero else round(((seed * 17 + len(path)) % 1000) / 7, 6)
    if t == "boolean":
        return False if zero else bool(seed % 2)
    if t == "array":
        if zero:
            return []
        return [value(schema["items"], path + "[]", seed + i, depth + 1, False) for i in range(2)]
    if t == "object":
        if "additionalProperties" in schema:
            if zero:
                return {}
            return {f"k{c}": value(schema["additionalProperties"], path + "{}", seed + i, depth + 1, False) for i, c in enumerate("ba")}
        return {k: value(v, f"{path}.{k}", seed, depth + 1, zero) for k, v in schema.get("properties", {}).items()}
    return None


def obj(ref, path, seed, depth, zero):
    props = SCHEMAS[ref].get("properties", {})
    return {k: value(v, f"{path}.{k}" if path else k, seed, depth, zero) for k, v in props.items()}


def methods(resources):
    for r in resources.values():
        yield from r.get("methods", {}).values()
        yield from methods(r.get("resources", {}))


def key(method, path):
    path = path.replace("youtube/v3/", "")
    if path.startswith("captions/"):
        path = "captions/_"
    return f"{method}_{path.replace('/', '.')}"


def list_fixture(resp_ref):
    item_ref = SCHEMAS[resp_ref]["properties"]["items"]["items"]["$ref"]
    pages = []
    for p in range(3):
        items = [obj(item_ref, "", p * 10 + i, 0, False) for i in range(3)]
        items.append(obj(item_ref, "", 0, 0, True) | {"id": f"zero-{p}"} if "id" in SCHEMAS[item_ref].get("properties", {}) else obj(item_ref, "", 0, 0, True))
        page = {"kind": f"youtube#{resp_ref[0].lower()}{resp_ref[1:]}", "etag": f"etag-page-{p}", "items": items}
        if "pageInfo" in SCHEMAS[resp_ref]["properties"]:
            page["pageInfo"] = {"totalResults": 12, "resultsPerPage": 4}
        if p < 2 and "nextPageToken" in SCHEMAS[resp_ref]["properties"]:
            page["nextPageToken"] = f"p{p + 2}"
        page["_token"] = "" if p == 0 else f"p{p + 1}"
        pages.append(page)
    return {"pages": pages}


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    written = 0
    for m in methods(SPEC["resources"]):
        resp = m.get("response", {}).get("$ref")
        k = key(m["httpMethod"], m.get("flatPath", m["path"]))
        if resp and "items" in SCHEMAS[resp].get("properties", {}) and SCHEMAS[resp]["properties"]["items"].get("type") == "array":
            fx = list_fixture(resp)
        elif resp:
            fx = obj(resp, "", 1, 0, False)
        elif m["id"] == "youtube.captions.download":
            fx = {"raw": "1\n00:00:00,000 --> 00:00:01,000\nHello caption\n", "ctype": "text/plain"}
        else:
            continue
        if "mediaUpload" in m:
            (OUT / f"{key(m['httpMethod'], m['mediaUpload']['protocols']['simple']['path'].lstrip('/').replace('youtube/v3/', ''))}.json").write_text(json.dumps(fx, indent=1, ensure_ascii=False) + "\n")
            written += 1
        (OUT / f"{k}.json").write_text(json.dumps(fx, indent=1, ensure_ascii=False) + "\n")
        written += 1
    print(f"fixtures: {written} written", file=sys.stderr)


if __name__ == "__main__":
    main()
