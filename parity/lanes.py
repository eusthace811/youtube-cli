"""Live verification lanes per PR. Each lane runs real binaries, saves its
capture under docs/evidence/<pr>/, and reports PASS or FAIL against the pass
rule written in the plan.

    python3 -I parity/lanes.py r0
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "parity"))
import diff as differ  # noqa: E402
import run as runner  # noqa: E402
from recorder import Recorder  # noqa: E402

GO = str(ROOT / "ref" / "yutu-go")
HARNESS = str(ROOT / "ref" / "yutu-go-harness")
HEAD = str(ROOT / "target" / "release" / "yutu")
LANES = {}


def lane(pr, n, slug):
    def wrap(fn):
        LANES.setdefault(pr, []).append((n, slug, fn))
        return fn
    return wrap


def sh(cmd, **kw):
    p = subprocess.run(cmd, capture_output=True, text=True, shell=isinstance(cmd, str), **kw)
    return p.returncode, p.stdout, p.stderr


def fmt(cmd, code, out, err):
    return f"$ {cmd if isinstance(cmd, str) else ' '.join(cmd)}\nexit: {code}\n--- stdout ---\n{out}\n--- stderr ---\n{err}\n"


def case(cid):
    for f in (ROOT / "parity" / "corpus").glob("*.json"):
        for c in json.loads(f.read_text()):
            if c["id"] == cid:
                return c
    raise KeyError(cid)


def run_one(binary, c, rec=None):
    own = rec is None
    rec = rec or Recorder()
    try:
        return runner.run_case(binary, c, rec)
    finally:
        if own:
            rec.close()


def show(r):
    reqs = "\n".join(json.dumps(x, ensure_ascii=False)[:600] for x in r["requests"])
    return f"exit: {r['exit']}\n--- requests ({len(r['requests'])}) ---\n{reqs}\n--- stdout ---\n{r['stdout'][:4000]}\n--- stderr ---\n{r['stderr'][:2000]}\n"


# R0 ---------------------------------------------------------------------

@lane("r0", 1, "r0-l1-video-list.txt")
def r0_l1():
    c = {"id": "lane", "argv": ["video", "list", "--ids", "a,b", "-o", "json"]}
    r = run_one(HARNESS, c)
    req = r["requests"][0] if r["requests"] else {}
    fixture = json.loads((ROOT / "parity/fixtures/GET_videos.json").read_text())["pages"][0]["items"]
    ok = (req.get("method") == "GET" and req.get("path") == "/youtube/v3/videos" and req["query"].get("id") == ["a", "b"]
          and len(json.loads(r["stdout"])) == len(fixture) and r["exit"] == 0)
    return ok, "Trunk has no harness; this lane gates the added behavior on ref/yutu-go-harness.\n" + show(r)


@lane("r0", 2, "r0-l2-gate.txt")
def r0_l2():
    r = run_one(HARNESS, case("cli.playlist.insert.no-yes"))
    return r["exit"] == 1 and "operation not confirmed" in r["stderr"] and not r["requests"], show(r)


@lane("r0", 3, "r0-l3-upload.txt")
def r0_l3():
    r = run_one(HARNESS, case("cli.video.insert.defaults"))
    golden = json.loads((ROOT / "parity/golden/cases/cli.video.insert.defaults.json").read_text())
    req = r["requests"][0]
    ok = (req["path"] == "/upload/youtube/v3/videos" and req["query"].get("uploadType") == ["multipart"]
          and differ.compare(golden, r, {"id": "x"}) == [])
    return ok, show(r)


@lane("r0", 4, "r0-l4-paging.txt")
def r0_l4():
    r = run_one(HARNESS, case("cli.video.list.maxResults.0"))
    tokens = [q["query"].get("pageToken", [""])[0] for q in r["requests"]]
    return tokens == ["", "p2", "p3"], f"page tokens: {tokens}\n" + show(r)


@lane("r0", 5, "r0-l5-contract.txt")
def r0_l5():
    code, out, err = sh([sys.executable, "-I", "parity/contract.py", "--bin", GO, "--compare", "parity/golden/mcp-contract.json"], cwd=ROOT)
    tools = len(json.loads((ROOT / "parity/golden/mcp-contract.json").read_text())["tools"]["result"]["tools"])
    return code == 0 and tools == 74 and "contract: 0 differences" in out, f"tools: {tools}\n" + fmt("contract.py --compare", code, out, err)


@lane("r0", 6, "r0-l6-differ.txt")
def r0_l6():
    scratch = Path(tempfile.mkdtemp())
    shutil.copytree(ROOT / "parity/fixtures", scratch / "fixtures")
    f = scratch / "fixtures/GET_videos.json"
    f.write_text(f.read_text().replace('"id-0"', '"id-X"', 1))
    rec = Recorder(scratch / "fixtures")
    c = case("cli.video.list.defaults")
    r = runner.run_case(HARNESS, c, rec)
    rec.close()
    golden = json.loads((ROOT / "parity/golden/cases/cli.video.list.defaults.json").read_text())
    found = differ.compare(golden, r, c)
    return len(found) == 1 and found[0].startswith("stdout"), "edited one fixture byte (id-0 -> id-X)\n" + "\n".join(found)


@lane("r0", 7, "r0-l7-determinism.txt")
def r0_l7():
    code, out, err = sh([sys.executable, "-I", "parity/run.py", "--bin", HARNESS, "--all", "--compare"], cwd=ROOT)
    return code == 0 and out.strip().endswith("0 differences"), out[-3000:] + err[-2000:]


@lane("r0", 8, "r0-l8-rebuild.txt")
def r0_l8():
    d = Path(tempfile.mkdtemp())
    branch = sh(["git", "-C", str(ROOT), "rev-parse", "--abbrev-ref", "HEAD"])[1].strip()
    sh(["git", "clone", "--quiet", "--branch", branch, str(ROOT), str(d / "clone")])
    ref = d / "clone" / "ref"
    code, out, err = sh(["bash", str(ref / "build.sh")], env={**os.environ, "GO": str(ROOT / ".tools/go/bin/go"), "GOPATH": str(ROOT / ".tools/gopath")})
    want = {l.split()[1]: l.split()[0] for l in (ROOT / "ref/SHA256SUMS").read_text().splitlines()}
    got = {l.split()[1]: l.split()[0] for l in out.splitlines() if len(l.split()) == 2 and l.startswith(tuple("0123456789abcdef"))}
    ok = code == 0 and "all modules verified" in out + err and got.get("yutu-go-harness") == want["yutu-go-harness"]
    return ok, fmt("ref/build.sh (clean clone)", code, out, err) + f"\nexpected: {want}\ngot: {got}\n"


@lane("r0", 9, "r0-l9-version.txt")
def r0_l9():
    code, out, err = sh([GO, "version"], env={"PATH": "/usr/bin:/bin"})
    return code == 0 and out == "" and "yutu v0.11.0" in err, fmt("yutu-go version", code, out, err)


@lane("r0", 10, "r0-l10-formats.txt")
def r0_l10():
    y = run_one(HARNESS, case("cli.video.list.output.yaml"))
    t = run_one(HARNESS, case("cli.video.list.output.table"))
    ok = ("agegating:" in y["stdout"] and "ageGating" not in y["stdout"] and y["stdout"].endswith("\n\n")
          and t["stdout"].startswith(" ID ") and not t["stdout"].endswith("\n") and "│" not in t["stdout"] and "┌" not in t["stdout"])
    return ok, "--- yaml (head) ---\n" + y["stdout"][:800] + "\n--- yaml tail repr ---\n" + repr(y["stdout"][-60:]) + "\n--- table ---\n" + t["stdout"] + "|<EOF\n"


# R1 ---------------------------------------------------------------------

def both(args, env=None):
    env = env or {"PATH": "/usr/bin:/bin", "HOME": "/tmp"}
    g = sh([GO] + args, env=env)
    h = sh([HEAD] + args, env=env)
    return g, h


@lane("r1", 1, "r1-l1-version.txt")
def r1_l1():
    g, h = both(["version"])
    return g == h, fmt("yutu-go version", *g) + fmt("yutu version", *h)


@lane("r1", 2, "r1-l2-help.txt")
def r1_l2():
    g, h = both(["--help"])
    return g == h, fmt("yutu-go --help", *g) + fmt("yutu --help", *h)


@lane("r1", 3, "r1-l3-unknown.txt")
def r1_l3():
    g, h = both(["nope"])
    return g[0] == h[0] and g[2] == h[2], fmt("yutu-go nope", *g) + fmt("yutu nope", *h)


@lane("r1", 4, "r1-l4-ban.txt")
def r1_l4():
    d = Path(tempfile.mkdtemp())
    sh(["git", "clone", "--quiet", "--branch", sh(["git", "-C", str(ROOT), "rev-parse", "--abbrev-ref", "HEAD"])[1].strip(), str(ROOT), str(d / "c")])
    toml = d / "c" / "crates" / "yutu-core" / "Cargo.toml"
    toml.write_text(toml.read_text().replace("[dependencies]\n", "[dependencies]\ntracing-opentelemetry = \"0.32\"\n", 1))
    code, out, err = sh(["cargo", "deny", "check", "bans"], cwd=d / "c")
    named = "tracing-opentelemetry" in out + err
    return code != 0 and named, fmt("cargo deny check bans (with tracing-opentelemetry added)", code, out[-3000:], err[-3000:])


@lane("r1", 5, "r1-l5-egress.txt")
def r1_l5():
    code, out, err = sh(["cargo", "run", "-q", "-p", "xtask", "--", "egress", HEAD], cwd=ROOT)
    return code == 0 and "egress: 0 unexpected hosts" in out + err, fmt("xtask egress target/release/yutu", code, out, err)


@lane("r1", 6, "r1-l6-offline.txt")
def r1_l6():
    profile = "(version 1)(allow default)(deny network*)"
    code, out, err = sh(["sandbox-exec", "-p", profile, HEAD, "version"], env={"PATH": "/usr/bin:/bin"})
    g = sh([GO, "version"], env={"PATH": "/usr/bin:/bin"})
    return code == 0 and err == g[2], fmt("sandbox-exec (deny network*) yutu version", code, out, err)


@lane("r1", 7, "r1-l7-repro.txt")
def r1_l7():
    hashes = []
    log = ""
    for i in range(2):
        target = Path(tempfile.mkdtemp()) / "target"
        code, out, err = sh(["cargo", "build", "--release", "--locked", "--target-dir", str(target), "-p", "yutu"], cwd=ROOT)
        h = sh(["shasum", "-a", "256", str(target / "release" / "yutu")])[1].split()[0] if code == 0 else "build failed"
        hashes.append(h)
        log += f"build {i + 1}: exit {code} sha256 {h}\n{err[-500:]}\n"
    return len(set(hashes)) == 1 and "failed" not in hashes[0], log


@lane("r1", 8, "r1-l8-help-sub.txt")
def r1_l8():
    g, h = both(["help", "video"])
    return g == h, fmt("yutu-go help video", *g) + fmt("yutu help video", *h)


@lane("r1", 9, "r1-l9-unsafe.txt")
def r1_l9():
    roots = sorted(str(p.relative_to(ROOT)) for p in ROOT.glob("crates/*/src/lib.rs")) + sorted(str(p.relative_to(ROOT)) for p in ROOT.glob("crates/*/src/main.rs")) + ["xtask/src/main.rs"]
    missing = [r for r in roots if "#![forbid(unsafe_code)]" not in (ROOT / r).read_text()]
    grep = sh("grep -rn 'unsafe' crates/ xtask/ --include=*.rs | grep -v 'forbid(unsafe_code)' || true", cwd=ROOT)[1]
    code, out, err = sh(["cargo", "build", "-q", "--workspace"], cwd=ROOT)
    return not missing and not grep.strip() and code == 0, f"crate roots: {roots}\nmissing forbid: {missing}\nother unsafe mentions:\n{grep}\ncargo build exit {code}\n{err[-1000:]}"


@lane("r1", 10, "r1-l10-ci.txt")
def r1_l10():
    steps = [["cargo", "fmt", "--all", "--check"], ["cargo", "clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"],
             ["cargo", "test", "--workspace", "--locked"], ["cargo", "build", "--release", "--locked"],
             ["cargo", "run", "--locked", "-p", "xtask", "--", "egress", "target/release/yutu"], ["cargo", "deny", "check"]]
    log, ok = "SUBSTITUTED: no GitHub repository exists (operator chose a local repo), so the ci.yml steps run locally on macOS; the Linux job cannot run here (no Docker).\n", True
    for s in steps:
        code, out, err = sh(s, cwd=ROOT)
        ok &= code == 0
        log += f"$ {' '.join(s)} -> exit {code}\n{(out + err)[-600:]}\n"
    return ok, log


def main():
    pr = sys.argv[1]
    only = set(sys.argv[2:])
    out = ROOT / "docs" / "evidence" / pr
    out.mkdir(parents=True, exist_ok=True)
    failed = 0
    for n, slug, fn in LANES[pr]:
        if only and str(n) not in only:
            continue
        try:
            ok, text = fn()
        except Exception as e:  # a crashed lane is a failed lane
            ok, text = False, f"lane crashed: {e!r}"
        verdict = "PASS" if ok else "FAIL"
        (out / slug).write_text(f"{pr} lane {n}: {verdict}\n\n{text}")
        print(f"{pr} lane {n:2} {verdict}  {slug}")
        failed += not ok
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
