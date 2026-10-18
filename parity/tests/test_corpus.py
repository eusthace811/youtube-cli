import json
from pathlib import Path

PARITY = Path(__file__).resolve().parent.parent
CONTRACT = json.loads((PARITY / "golden" / "mcp-contract.json").read_text())
CASES = [c for f in sorted((PARITY / "corpus").glob("*.json")) for c in json.loads(f.read_text())]


def test_every_tool_has_a_case():
    called = {m["params"]["name"] for c in CASES for m in c.get("messages", []) if m.get("method") == "tools/call"}
    tools = {t["name"] for t in CONTRACT["tools"]["result"]["tools"]}
    assert len(tools) == 74
    assert tools - called == set()


def test_every_input_property_is_exercised():
    seen = {(m["params"]["name"], p) for c in CASES for m in c.get("messages", [])
            if m.get("method") == "tools/call" for p in m["params"]["arguments"]}
    missing = [(t["name"], p) for t in CONTRACT["tools"]["result"]["tools"]
               for p in t["inputSchema"].get("properties", {}) if (t["name"], p) not in seen]
    assert missing == []


def test_corpus_is_large_enough_and_ids_unique():
    ids = [c["id"] for c in CASES]
    assert len(ids) >= 600
    assert len(ids) == len(set(ids))


def test_every_gated_command_has_a_yes_pair():
    gated = {c["group"] for c in CASES if c["id"].endswith(".no-yes")}
    assert len(gated) == 51
