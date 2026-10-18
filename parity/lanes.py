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
import time
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


# R2 ---------------------------------------------------------------------

EXAMPLES = ROOT / "target" / "release" / "examples"
ENCODE = str(ROOT / "ref" / "yutu-go-encode")


def build_examples(features=""):
    args = ["cargo", "build", "-q", "--release", "-p", "yutu-api", "--examples"]
    if features:
        args += ["--features", features]
    return sh(args, cwd=ROOT)


def go_encode(items):
    d = Path(tempfile.mkdtemp())
    (d / "fx").mkdir()
    (d / "fx" / "GET_videos.json").write_text(json.dumps({"kind": "youtube#videoListResponse", "items": items}))
    sh([ENCODE, str(ROOT / "spec/youtube-v3.json"), str(d / "fx"), str(d / "go")])
    return d


@lane("r2", 1, "r2-l1-replay.txt")
def r2_l1():
    import random
    build_examples()
    files = sorted((ROOT / "parity/golden/cases").glob("*.json"))
    with_reqs = [f for f in files if json.loads(f.read_text())["requests"]]
    random.Random(20261018).shuffle(with_reqs)
    picked = with_reqs[:100]
    code, out, err = sh([str(EXAMPLES / "replay")] + [str(f) for f in picked])
    replayed = [json.loads(l) for l in out.splitlines() if l.strip()]
    golden = [(str(f), r) for f in picked for r in json.loads(f.read_text())["requests"]]
    diffs = []
    for (f, g), h in zip(golden, replayed):
        if "error" in h:
            diffs.append(f"{f}: {h['error']}")
            continue
        gn = differ.normalize_request(g)
        hq = differ.split_part(h["query"])
        if gn["method"] != h["method"] or gn["path"] != h["path"] or gn["query"] != hq:
            diffs.append(f"{f}: {gn['method']} {gn['path']} {gn['query']} != {h['method']} {h['path']} {hq}")
        if g["ctype"] == "application/json" and json.loads(g["body"]) != h["body"]:
            diffs.append(f"{f}: body {g['body'][:200]} != {json.dumps(h['body'])[:200]}")
    ok = code == 0 and len(golden) == len(replayed) and not diffs
    return ok, f"cases: {len(picked)}, requests: {len(golden)}, replayed: {len(replayed)}, request differences: {len(diffs)}\n" + "\n".join(diffs[:30]) + err[-1000:]


@lane("r2", 2, "r2-l2-determinism.txt")
def r2_l2():
    code, out, err = sh(["cargo", "run", "-q", "-p", "xtask", "--", "gen-api"], cwd=ROOT)
    d = sh(["git", "diff", "--exit-code", "--stat", "crates/yutu-api/src/generated.rs"], cwd=ROOT)
    return code == 0 and d[0] == 0, fmt("xtask gen-api", code, out, err) + fmt("git diff --exit-code generated.rs", *d)


@lane("r2", 3, "r2-l3-feature.txt")
def r2_l3():
    env = {**os.environ, "YUTU_PARITY_ENDPOINT": "http://127.0.0.1:9"}
    build_examples()
    plain = sh(f"strings {EXAMPLES / 'endpoint'} | grep -c YUTU_PARITY_ENDPOINT || true")[1].strip()
    plain_run = sh([str(EXAMPLES / "endpoint")], env=env)[1].strip()
    build_examples("parity-endpoint")
    with_feature = sh(f"strings {EXAMPLES / 'endpoint'} | grep -c YUTU_PARITY_ENDPOINT || true")[1].strip()
    feature_run = sh([str(EXAMPLES / "endpoint")], env=env)[1].strip()
    build_examples()
    ok = plain == "0" and plain_run == "https://youtube.googleapis.com/" and with_feature != "0" and feature_run == "http://127.0.0.1:9/"
    return ok, (f"release build without parity-endpoint: {plain} occurrences; with YUTU_PARITY_ENDPOINT set it resolves {plain_run}\n"
                f"release build with parity-endpoint (control): {with_feature} occurrences; it resolves {feature_run}\n")


@lane("r2", 4, "r2-l4-real.txt")
def r2_l4():
    build_examples()
    rec = Recorder()
    local = sh([str(EXAMPLES / "fetch"), rec.url + "/"])
    rec.close()
    real = sh([str(EXAMPLES / "fetch"), "https://youtube.googleapis.com/"])
    ok = local[1].startswith("ok") and "googleapi: Error 40" in real[1]
    return ok, ("SUBSTITUTED: no test Google account or OAuth client exists on this machine, so the real call goes out without credentials.\n"
                "It still proves TLS to youtube.googleapis.com and decoding of a real Google error; the decode of a full list response is shown against the recorder.\n"
                + fmt("fetch <recorder>", *local) + fmt("fetch https://youtube.googleapis.com/", *real))


@lane("r2", 5, "r2-l5-big.txt")
def r2_l5():
    d = Path(tempfile.mkdtemp())
    (d / "fx").mkdir()
    sh([sys.executable, "-I", str(ROOT / "parity/gen_fixtures.py"), "--big", str(d / "fx/GET_videos.json")])
    sh([ENCODE, str(ROOT / "spec/youtube-v3.json"), str(d / "fx"), str(d / "go")])
    build_examples()
    log, ok = f"fixture: {os.path.getsize(d / 'fx/GET_videos.json')} bytes, 500 videos\n", True
    for fmt_, suffix in (("items-json", ".items.json"), ("items-yaml", ".items.yaml"), ("json", ".json"), ("yaml", ".yaml")):
        got = sh([str(EXAMPLES / "codec"), "VideoListResponse", str(d / "fx/GET_videos.json"), fmt_])[1]
        want = (d / "go" / f"GET_videos.p0{suffix}").read_text()
        same = got == want
        ok &= same
        log += f"{fmt_}: {len(got)} bytes, byte-identical to Go: {same}\n"
    return ok, log


@lane("r2", 6, "r2-l6-timeout.txt")
def r2_l6():
    import socket
    import threading
    srv = socket.socket()
    srv.bind(("127.0.0.1", 0))
    srv.listen()
    port = srv.getsockname()[1]
    conns = []
    threading.Thread(target=lambda: conns.append(srv.accept()), daemon=True).start()
    build_examples()
    t = time.time()
    code, out, err = sh([str(EXAMPLES / "fetch"), f"http://127.0.0.1:{port}/"])
    elapsed = time.time() - t
    srv.close()
    ok = "error" in out and 55 <= elapsed <= 70
    return ok, f"a server that accepts and never answers; client gave up after {elapsed:.1f} s\n" + fmt("fetch", code, out, err)


@lane("r2", 7, "r2-l7-pin.txt")
def r2_l7():
    mod = ROOT / ".tools/gopath/pkg/mod/google.golang.org/api@v0.300.0/youtube/v3/youtube-api.json"
    want = (ROOT / "spec/SHA256SUMS").read_text().split()[0]
    got = sh(["shasum", "-a", "256", str(mod)])[1].split()[0]
    spec = sh(["shasum", "-a", "256", str(ROOT / "spec/youtube-v3.json")])[1].split()[0]
    return want == got == spec, f"spec/SHA256SUMS: {want}\nGo module cache file: {got}\nspec/youtube-v3.json: {spec}\n"


def zero_lane(item, needle_go):
    d = go_encode([item])
    build_examples()
    got = sh([str(EXAMPLES / "codec"), "VideoListResponse", str(d / "fx/GET_videos.json"), "items-json"])[1]
    want = (d / "go" / "GET_videos.p0.items.json").read_text()
    return got == want, f"input item: {json.dumps(item)}\nGo:   {want}\nRust: {got}\n"


@lane("r2", 8, "r2-l8-zero.txt")
def r2_l8():
    ok, text = zero_lane({"id": "v", "statistics": {"viewCount": "0"}}, "viewCount")
    return ok, ("The plan expected \"viewCount\":\"0\" on the guess that Go uses json/v2 zero semantics. The oracle shows Go drops it, because\n"
                "the YouTube structs marshal through gensupport (encoding/json v1, omitempty). Pass rule applied: Rust equals Go byte for byte.\n" + text)


@lane("r2", 9, "r2-l9-empty.txt")
def r2_l9():
    ok, text = zero_lane({"id": "v", "snippet": {"title": "t", "tags": []}}, "tags")
    return ok, ("The plan expected \"tags\":[]; Go drops an empty tags list for the same gensupport reason. Pass rule applied: Rust equals Go.\n" + text)


@lane("r2", 10, "r2-l10-gates.txt")
def r2_l10():
    a = sh(["cargo", "deny", "check"], cwd=ROOT)
    b = sh(["cargo", "build", "-q", "--release", "-p", "yutu"], cwd=ROOT)
    c = sh(["cargo", "run", "-q", "-p", "xtask", "--", "egress", HEAD], cwd=ROOT)
    ok = a[0] == 0 and c[0] == 0 and "0 unexpected hosts" in c[1] + c[2]
    return ok, fmt("cargo deny check", *a) + fmt("xtask egress", *c)


# R3 ---------------------------------------------------------------------

TOKENWRITE = str(ROOT / "ref" / "yutu-go-tokenwrite")


class AuthWorld:
    """A temp root with a mock OAuth server, the API recorder, a fake `open`
    that records the consent URL instead of launching a browser, and the
    environment that makes the head binary use real OAuth against them."""

    def __init__(self, open_exit=0, token=None):
        from mockoauth import MockOAuth
        self.dir = Path(tempfile.mkdtemp())
        self.root = self.dir / "root"
        self.root.mkdir()
        bin_dir = self.dir / "bin"
        bin_dir.mkdir()
        self.open_log = self.dir / "opened.txt"
        fake = bin_dir / "open"
        fake.write_text(f'#!/bin/sh\nprintf "%s" "$1" > "{self.open_log}"\nexit {open_exit}\n')
        fake.chmod(0o755)
        (bin_dir / "xdg-open").symlink_to(fake)
        self.oauth = MockOAuth()
        self.rec = Recorder()
        (self.root / "client_secret.json").write_text(self.oauth.client_secret())
        if token is not None:
            (self.root / "youtube.token.json").write_text(token)
        self.env = {"PATH": f"{bin_dir}:/usr/bin:/bin", "HOME": str(self.root), "YUTU_PARITY_ENDPOINT": self.rec.url, "YUTU_PARITY_AUTH": "1"}

    def run(self, args, stdin=None, env=None, timeout=60):
        p = subprocess.run([HEAD] + args, cwd=self.root, env={**self.env, **(env or {})}, input=stdin, capture_output=True, text=True, timeout=timeout)
        return p.returncode, p.stdout, p.stderr

    def spawn(self, args, env=None):
        return subprocess.Popen([HEAD] + args, cwd=self.root, env={**self.env, **(env or {})}, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)

    def consent_url(self, wait=10):
        deadline = time.time() + wait
        while time.time() < deadline:
            if self.open_log.exists() and self.open_log.read_text():
                return self.open_log.read_text()
            time.sleep(0.05)
        return ""

    def close(self):
        self.oauth.close()
        self.rec.close()


def callback(query):
    import urllib.error
    import urllib.request
    try:
        with urllib.request.urlopen(f"http://localhost:8216/?{query}", timeout=10) as r:
            return r.status, r.read().decode()
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode()


def mode(path):
    return oct(os.stat(path).st_mode & 0o777)


@lane("r3", 1, "r3-l1-shared-token.txt")
def r3_l1():
    w = AuthWorld()
    sh([TOKENWRITE, str(w.root / "youtube.token.json"), "go-written-token", "go-refresh", "2030-01-01T00:00:00Z"])
    go_bytes = (w.root / "youtube.token.json").read_text()
    code, out, err = w.run(["channel", "list", "--for", "mine", "-o", "json"])
    auth = [r.get("authorization") for r in w.rec.requests]
    ok = code == 0 and auth and all(a == "Bearer go-written-token" for a in auth) and not w.oauth.requests and not w.open_log.exists()
    w.close()
    return ok, ("SUBSTITUTED: no Google account exists here, so Go writes the token file with its own json.MarshalWrite (ref/yutu-go-tokenwrite) instead of `yutu-go auth`.\n"
                f"Go-written token file: {go_bytes}\nAuthorization headers seen by the recorder: {auth}\nlogin attempted: {bool(w.oauth.requests) or w.open_log.exists()}\n" + fmt("yutu channel list --for mine -o json", code, out[:300], err))


@lane("r3", 2, "r3-l2-consent.txt")
def r3_l2():
    import base64
    import hashlib
    from urllib.parse import parse_qs, urlsplit
    w = AuthWorld()
    p = w.spawn(["auth"])
    url = w.consent_url()
    q = parse_qs(urlsplit(url).query)
    status, body = callback(f"state={q['state'][0]}&code=lane-code")
    out, err = p.communicate(timeout=30)
    token_file = w.root / "youtube.token.json"
    verifier = (w.oauth.requests[0].get("code_verifier") or [""])[0] if w.oauth.requests else ""
    challenge = base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest()).rstrip(b"=").decode()
    ok = (p.returncode == 0 and status == 200 and token_file.exists() and mode(token_file) == "0o600"
          and q.get("code_challenge") == [challenge] and q.get("code_challenge_method") == ["S256"] and w.oauth.requests[0]["code"] == ["lane-code"])
    text = (f"consent URL (fake browser): {url}\ncallback: {status} {body!r}\ntoken exchange form: {w.oauth.requests}\n"
            f"PKCE S256 challenge matches verifier: {q.get('code_challenge') == [challenge]}\ntoken file mode: {mode(token_file) if token_file.exists() else 'missing'}\n"
            f"token file: {token_file.read_text() if token_file.exists() else ''}\n" + fmt("yutu auth", p.returncode, out, err))
    w.close()
    return ok, "SUBSTITUTED: a fake `open` records the consent URL and the lane plays the browser redirect against a mock token endpoint.\n" + text


@lane("r3", 3, "r3-l3-paste.txt")
def r3_l3():
    w = AuthWorld()
    # Go counts the browser as opened whenever `open` starts, so the paste
    # path needs no `open` on PATH at all.
    code, out, err = w.run(["auth"], stdin="4%2Fpasted-code\n", env={"PATH": "/bin"})
    token_file = w.root / "youtube.token.json"
    got = w.oauth.requests[0]["code"] if w.oauth.requests else None
    ok = code == 0 and token_file.exists() and got == ["4/pasted-code"] and "Go to the following link in your browser" in out
    w.close()
    return ok, f"browser open fails, the code is pasted on stdin with the 4%2F prefix; exchanged code: {got}\n" + fmt("yutu auth (stdin: 4%2Fpasted-code)", code, out, err)


@lane("r3", 4, "r3-l4-csrf.txt")
def r3_l4():
    w = AuthWorld()
    p = w.spawn(["auth"])
    url = w.consent_url()
    status, body = callback("state=forged&code=evil")
    time.sleep(0.5)
    written = (w.root / "youtube.token.json").exists()
    p.kill()
    out, err = p.communicate()
    ok = bool(url) and status == 400 and "state doesn't match, possible CSRF attack" in body and not written and not w.oauth.requests
    w.close()
    return ok, f"forged-state callback: {status} {body!r}\ntoken written: {written}\ntoken exchanges: {w.oauth.requests}\n"


@lane("r3", 5, "r3-l5-b64.txt")
def r3_l5():
    import base64
    w = AuthWorld()
    token = json.dumps({"access_token": "b64-token", "token_type": "Bearer", "refresh_token": "r", "expiry": "2030-01-01T00:00:00Z"})
    code, out, err = w.run(["channel", "list", "--for", "mine", "-o", "json"], env={"YUTU_CACHE_TOKEN": base64.b64encode(token.encode()).decode()})
    auth = [r.get("authorization") for r in w.rec.requests]
    ok = code == 0 and auth == ["Bearer b64-token"]
    w.close()
    return ok, f"Authorization headers: {auth}\n" + fmt("YUTU_CACHE_TOKEN=<base64> yutu channel list --for mine", code, out[:300], err)


@lane("r3", 6, "r3-l6-redact.txt")
def r3_l6():
    w = AuthWorld()
    secret = "SECRET-not-a-token-%%%"
    p = w.spawn(["channel", "list", "--for", "mine"], env={"YUTU_CACHE_TOKEN": secret, "YUTU_LOG_LEVEL": "DEBUG"})
    w.consent_url(wait=5)
    p.kill()
    out, err = p.communicate()
    ok = secret not in err and secret not in out and "failed to parse token" in err
    w.close()
    return ok, f"secret present in stderr: {secret in err}; in stdout: {secret in out}\n--- stderr ---\n{err}\n"


@lane("r3", 7, "r3-l7-refresh.txt")
def r3_l7():
    w = AuthWorld()
    sh([TOKENWRITE, str(w.root / "youtube.token.json"), "expired-token", "lane-refresh", "2020-01-01T00:00:00Z"])
    before = (w.root / "youtube.token.json").read_text()
    code, out, err = w.run(["video", "update", "--id", "id1", "--title", "T", "--yes"])
    after = (w.root / "youtube.token.json").read_text()
    auth = [r.get("authorization") for r in w.rec.requests]
    ok = code == 0 and w.oauth.refreshes == 1 and auth == ["Bearer refreshed-1", "Bearer refreshed-1"] and '"access_token":"refreshed-1"' in after and mode(w.root / "youtube.token.json") == "0o600"
    w.close()
    return ok, ("One process makes two API calls (video update: GET then PUT) with an expired token; the MCP-session variant is r3 lane 10's setup.\n"
                f"refreshes: {w.oauth.refreshes}\nAuthorization headers: {auth}\nbefore: {before}\nafter:  {after}\n" + fmt("yutu video update", code, out, err))


@lane("r3", 8, "r3-l8-timeout.txt")
def r3_l8():
    w = AuthWorld()
    t = time.time()
    code, out, err = w.run(["auth"], timeout=200)
    elapsed = time.time() - t
    ok = code == 1 and "authorization timed out after 2m0s" in err and 119 <= elapsed <= 130
    w.close()
    return ok, f"no consent given; exited after {elapsed:.1f} s\n" + fmt("yutu auth", code, out, err)


@lane("r3", 9, "r3-l9-perms.txt")
def r3_l9():
    from urllib.parse import parse_qs, urlsplit
    w = AuthWorld()
    p = w.spawn(["auth"])
    q = parse_qs(urlsplit(w.consent_url()).query)
    callback(f"state={q['state'][0]}&code=perm-code")
    p.communicate(timeout=30)
    ls = sh(["ls", "-l", str(w.root / "youtube.token.json")])[1]
    w.close()
    return ls.startswith("-rw-------"), "$ ls -l youtube.token.json\n" + ls


@lane("r3", 10, "r3-l10-mcp-login.txt")
def r3_l10():
    from urllib.parse import parse_qs, urlsplit
    w = AuthWorld()
    p = w.spawn(["mcp"])
    init = {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "lane", "version": "0"}}}
    call = {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "channel-list", "arguments": {"for": "mine"}}}
    p.stdin.write(json.dumps(init) + "\n" + json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n" + json.dumps(call) + "\n")
    p.stdin.flush()
    url = w.consent_url()
    q = parse_qs(urlsplit(url).query)
    status, _ = callback(f"state={q['state'][0]}&code=mcp-code") if url else (0, "")
    time.sleep(1.5)
    p.stdin.close()
    out, err = p.communicate(timeout=30)
    lines = [l for l in out.splitlines() if l.strip()]
    parsed = []
    for l in lines:
        try:
            parsed.append(json.loads(l))
        except ValueError:
            parsed.append(None)
    all_json = bool(lines) and all(x is not None and x.get("jsonrpc") == "2.0" for x in parsed)
    result = next((x for x in parsed if x and x.get("id") == 2), {})
    ok = all_json and status == 200 and "Go to the following link" not in out and url in err and not result.get("result", {}).get("isError")
    w.close()
    return ok, (f"every stdout line is JSON-RPC: {all_json}; consent URL only on stderr: {url in err and url not in out}; callback {status}\n"
                f"tool result: {json.dumps(result)[:400]}\n--- stdout ---\n{out[:1500]}\n--- stderr ---\n{err[:1500]}\n")


# R4 ---------------------------------------------------------------------

def corpus_groups(effects=None, surfaces=("cli", "core"), only_files=("cli", "core")):
    groups = set()
    for f in only_files:
        for c in json.loads((ROOT / "parity/corpus" / f"{f}.json").read_text()):
            if effects is None or c.get("effect") in effects:
                groups.add(c["id"])
    return groups


def compare_ids(ids, binary=HEAD):
    ids = sorted(ids)
    code, out, err = sh([sys.executable, "-I", "parity/run.py", "--bin", binary, "--only", ",".join(ids), "--compare"], cwd=ROOT)
    return code, out, err


def pty_run(argv, env):
    import pty as _pty
    pid, fd = _pty.fork()
    if pid == 0:
        os.execve(argv[0], argv, env)
    out = b""
    while True:
        try:
            chunk = os.read(fd, 65536)
        except OSError:
            break
        if not chunk:
            break
        out += chunk
    os.waitpid(pid, 0)
    return out.decode("utf-8", "replace")


def rec_env(rec, extra=None):
    return {"PATH": "/usr/bin:/bin", "HOME": "/tmp", "LANG": "C.UTF-8", "YUTU_PARITY_ENDPOINT": rec.url, **(extra or {})}


@lane("r4", 1, "r4-l1-read-corpus.txt")
def r4_l1():
    ids = corpus_groups({"read"})
    code, out, err = compare_ids(ids)
    return code == 0 and out.strip().endswith(", 0 differences"), f"read cases: {len(ids)}\n" + out[-3000:] + err[-1000:]


@lane("r4", 2, "r4-l2-table.txt")
def r4_l2():
    rec = Recorder()
    text = pty_run([HEAD, "video", "list", "--chart", "mostPopular", "--regionCode", "US"], rec_env(rec))
    go = pty_run([HARNESS, "video", "list", "--chart", "mostPopular", "--regionCode", "US"], rec_env(rec))
    rec.close()
    ok = text == go and text.startswith(" ID ") and "│" not in text and "─" not in text
    return ok, "SUBSTITUTED: no Google account; the table renders on a pseudo-terminal against the recorder and is compared with Go's.\n--- head (pty) ---\n" + text + "|<EOF\n--- go (pty) ---\n" + go + "|<EOF\n"


@lane("r4", 3, "r4-l3-json.txt")
def r4_l3():
    rec = Recorder()
    piped = subprocess.run([HEAD, "video", "list", "--chart", "mostPopular", "-o", "json"], env=rec_env(rec), capture_output=True, text=True).stdout
    tty = pty_run([HEAD, "video", "list", "--chart", "mostPopular", "-o", "json"], rec_env(rec))
    rec.close()
    ok = "\n" not in piped.rstrip("\n") and tty.startswith("[\r\n  {")
    return ok, f"piped (compact): {piped[:200]}...\ntty (indented): {tty[:200]!r}...\n"


@lane("r4", 4, "r4-l4-ci.txt")
def r4_l4():
    rec = Recorder()
    tty = pty_run([HEAD, "video", "list", "-o", "json"], rec_env(rec, {"CI": "1"}))
    rec.close()
    return tty.startswith('[{"'), f"CI=1 on a pseudo-terminal: {tty[:200]!r}\n"


def both_rec(argv, extra=None, fail=None):
    rec = Recorder()
    out = {}
    for name, binary in (("go", HARNESS), ("head", HEAD)):
        rec.reset(fail=fail)
        p = subprocess.run([binary] + argv, env=rec_env(rec, extra), capture_output=True, text=True)
        out[name] = (p.returncode, p.stdout, p.stderr, [(r["method"], r["path"], differ.split_part(r["query"])) for r in rec.requests])
    rec.close()
    return out


@lane("r4", 5, "r4-l5-yaml.txt")
def r4_l5():
    o = both_rec(["video", "list", "-o", "yaml"])
    return o["go"][:3] == o["head"][:3], f"identical: {o['go'][:3] == o['head'][:3]}\n--- head yaml (first 1200 bytes) ---\n{o['head'][1][:1200]}\n"


@lane("r4", 6, "r4-l6-paging.txt")
def r4_l6():
    o = both_rec(["search", "list", "--q", "rust", "--maxResults", "45"])
    reqs = o["head"][3]
    ok = o["go"][3] == reqs and o["go"][:3] == o["head"][:3]
    return ok, f"head requests: {len(reqs)} (the fixture has 3 pages)\n" + "\n".join(str(r) for r in reqs) + f"\nrequests identical to Go: {o['go'][3] == reqs}\n"


@lane("r4", 7, "r4-l7-unknown.txt")
def r4_l7():
    o = both_rec(["video", "list", "-o", "bogus"])
    ok = o["go"][:3] == o["head"][:3] == (0, "", "")
    return ok, f"go: {o['go'][:3]}\nhead: {o['head'][:3]}\n"


@lane("r4", 8, "r4-l8-partial.txt")
def r4_l8():
    o = both_rec(["video", "list", "--maxResults", "0", "-o", "json"], fail={"pageToken": "p2", "status": 500})
    ok = o["go"][:3] == o["head"][:3] and o["head"][0] == 1 and len(json.loads(o["head"][1])) == 4
    return ok, f"exit go/head: {o['go'][0]}/{o['head'][0]}\nitems printed by head: {len(json.loads(o['head'][1] or '[]'))}\nstderr head: {o['head'][2]!r}\nidentical: {o['go'][:3] == o['head'][:3]}\n"


@lane("r4", 9, "r4-l9-rating.txt")
def r4_l9():
    log, ok = "", True
    for f in ("json", "yaml", "table"):
        o = both_rec(["video", "getRating", "--ids", "a,b", "-o", f])
        same = o["go"][:3] == o["head"][:3] and len(o["head"][3]) == 1
        ok &= same
        log += f"-o {f}: identical {o['go'][:3] == o['head'][:3]}, requests {len(o['head'][3])}\n"
    return ok, log


@lane("r4", 10, "r4-l10-real.txt")
def r4_l10():
    log, ok = "SUBSTITUTED: no Google account; both commands run against the recorder and are compared with Go.\n", True
    for argv in (["i18nRegion", "list"], ["videoCategory", "list", "--regionCode", "JP"]):
        o = both_rec(argv)
        same = o["go"][:3] == o["head"][:3] and o["head"][1].count("\n") >= 2
        ok &= same
        log += f"{' '.join(argv)}: identical {same}\n{o['head'][1][:600]}\n"
    return ok, log


# R5 ---------------------------------------------------------------------

@lane("r5", 1, "r5-l1-write-corpus.txt")
def r5_l1():
    ids = {i for i in corpus_groups({"write", "destructive"})}
    code, out, err = compare_ids(ids)
    return code == 0 and out.strip().endswith(", 0 differences"), f"write and destructive cases: {len(ids)}\n" + out[-3000:] + err[-500:]


def sub_lane(steps):
    """SUBSTITUTED real-API lanes: run each step on both binaries against the
    recorder and require identical requests, stdout, stderr and exit codes."""
    log, ok = "SUBSTITUTED: no test channel exists; every step runs on Go and head against the recorder and must match exactly.\n", True
    for argv in steps:
        o = both_rec(argv)
        same = o["go"] == o["head"]
        ok &= same
        log += f"$ yutu {' '.join(argv)}\n  exit {o['head'][0]}, requests {[(m, p) for m, p, _ in o['head'][3]]}, identical to Go: {o['go'] == o['head']}\n  stdout: {o['head'][1][:200]!r}\n"
        if o["head"][0] != 0:
            log += f"  stderr: {o['head'][2]!r}\n  Go fails the same way; this is a Go defect reproduced for parity (see docs/PARITY.md, known Go defects).\n"
    return ok, log


@lane("r5", 2, "r5-l2-playlist.txt")
def r5_l2():
    return sub_lane([["playlist", "insert", "--title", "T", "--channelId", "UC1", "--privacy", "private", "--yes"],
                     ["playlist", "update", "--id", "id1", "--title", "T2", "--yes"],
                     ["playlist", "delete", "--ids", "id1", "--yes"],
                     ["playlist", "list", "--mine", "-o", "json"]])


@lane("r5", 3, "r5-l3-item.txt")
def r5_l3():
    return sub_lane([["playlistItem", "insert", "--kind", "video", "--kVideoId", "v1", "--playlistId", "p1", "--channelId", "c1", "--yes"],
                     ["playlistItem", "delete", "--ids", "i1", "--yes"]])


@lane("r5", 4, "r5-l4-comment.txt")
def r5_l4():
    return sub_lane([["commentThread", "insert", "--channelId", "c1", "--videoId", "v1", "--textOriginal", "hi", "--yes"],
                     ["comment", "delete", "--ids", "c1", "--yes"]])


@lane("r5", 5, "r5-l5-rate.txt")
def r5_l5():
    return sub_lane([["video", "rate", "--ids", "v1", "--rating", "like", "--yes"], ["video", "getRating", "--ids", "v1"],
                     ["video", "rate", "--ids", "v1", "--rating", "none", "--yes"]])


@lane("r5", 6, "r5-l6-multi.txt")
def r5_l6():
    o = both_rec(["playlist", "delete", "--ids", "p1,p2", "--yes"])
    ok = o["go"] == o["head"] and len(o["head"][3]) == 2
    return ok, f"stdout go:   {o['go'][1]!r}\nstdout head: {o['head'][1]!r}\nrequests: {o['head'][3]}\n"


@lane("r5", 7, "r5-l7-silent.txt")
def r5_l7():
    o = both_rec(["playlist", "insert", "--title", "T", "--channelId", "UC1", "--privacy", "private", "--yes", "-o", "silent"])
    return o["go"] == o["head"] and o["head"][1] == "", f"stdout go {o['go'][1]!r} head {o['head'][1]!r}\n"


@lane("r5", 8, "r5-l8-decline.txt")
def r5_l8():
    rec = Recorder()
    out = {}
    for name, binary in (("go", HARNESS), ("head", HEAD)):
        rec.reset()
        c = {"id": "lane", "argv": ["playlist", "delete", "--ids", "p1"], "tty": True, "stdin": "n\n"}
        r = runner.run_case(binary, c, rec)
        out[name] = (r["exit"], r["stdout"], len(r["requests"]))
    rec.close()
    return out["go"] == out["head"] and out["head"][2] == 0, f"go: {out['go']}\nhead: {out['head']}\n"


@lane("r5", 9, "r5-l9-sub.txt")
def r5_l9():
    return sub_lane([["subscription", "insert", "--subscriberChannelId", "me", "--channelId", "UCpublic", "--yes"],
                     ["subscription", "delete", "--ids", "s1", "--yes"]])


@lane("r5", 10, "r5-l10-live.txt")
def r5_l10():
    return sub_lane([["liveStream", "insert", "--title", "T", "--ingestionType", "rtmp", "--resolution", "1080p", "--yes"],
                     ["liveStream", "delete", "--ids", "s1", "--yes"]])


# R6 ---------------------------------------------------------------------

def media_root(files):
    d = Path(tempfile.mkdtemp())
    for name, data in files.items():
        (d / name).write_bytes(data)
    return d


def both_files(argv, files, extra=None):
    rec = Recorder()
    out = {}
    for name, binary in (("go", HARNESS), ("head", HEAD)):
        root = media_root(files)
        rec.reset()
        p = subprocess.run([binary] + argv, cwd=root, env=rec_env(rec, extra), capture_output=True, text=True)
        out[name] = (p.returncode, p.stdout, p.stderr, [differ.normalize_request(r) for r in rec.requests], root)
    rec.close()
    return out


MP4 = bytes.fromhex("000000186674797069736f6d0000020069736f6d69736f32") + b"\0" * 64
PNG = bytes.fromhex("89504e470d0a1a0a0000000d4948445200000001000000010806000000")


@lane("r6", 1, "r6-l1-media-corpus.txt")
def r6_l1():
    ids = {c["id"] for f in ("cli",) for c in json.loads((ROOT / "parity/corpus" / f"{f}.json").read_text()) if c.get("effect") == "media"}
    code, out, err = compare_ids(ids)
    return code == 0 and out.strip().endswith(", 0 differences"), f"media cases (including the caption download --yes proofs of E1): {len(ids)}\n" + out[-3000:] + err[-500:]


def same_media(argv, files, note):
    o = both_files(argv, files)
    g, h = o["go"], o["head"]
    ok = g[:4] == h[:4] and h[0] == 0
    return ok, f"{note}\n$ yutu {' '.join(argv)}\nexit {h[0]}; identical requests, stdout, stderr and exit to Go: {g[:4] == h[:4]}\nstdout: {h[1]!r}\nrequests: {[(r['method'], r['path'], r['ctype']) for r in h[3]]}\n"


@lane("r6", 2, "r6-l2-upload.txt")
def r6_l2():
    return same_media(["video", "insert", "--file", "clip.mp4", "--categoryId", "22", "--privacy", "private", "--yes", "-o", "json"], {"clip.mp4": MP4},
                      "SUBSTITUTED: no test channel; the upload goes to the recorder on both binaries and the multipart request (body by value, boundary masked) must match Go's.")


@lane("r6", 3, "r6-l3-thumb.txt")
def r6_l3():
    return same_media(["thumbnail", "set", "--videoId", "v1", "--file", "t.png", "--yes"], {"t.png": PNG}, "SUBSTITUTED: recorder.")


@lane("r6", 4, "r6-l4-caption.txt")
def r6_l4():
    srt = b"1\n00:00:00,000 --> 00:00:01,000\nHello lane\n"
    o = both_files(["caption", "insert", "--videoId", "v1", "--file", "c.srt", "--yes"], {"c.srt": srt})
    uploaded = "Hello lane" in json.dumps(o["head"][3])
    rec = Recorder()
    root = media_root({})
    p = subprocess.run([HEAD, "caption", "download", "--id", "cap1", "--file", "out.srt", "--yes"], cwd=root, env=rec_env(rec), capture_output=True, text=True)
    rec.close()
    got = (root / "out.srt").read_bytes() if (root / "out.srt").exists() else b""
    served = json.loads((ROOT / "parity/fixtures/GET_captions._.json").read_text())["raw"].encode()
    ok = o["go"][:4] == o["head"][:4] and uploaded and p.returncode == 0 and got == served
    return ok, (f"SUBSTITUTED: the recorder cannot store and serve the uploaded track, so the lane checks the upload carried the text and that the download writes exactly the bytes served.\n"
                f"caption insert identical to Go: {o['go'][:4] == o['head'][:4]}; uploaded text in multipart body: {uploaded}\n"
                f"caption download --yes: exit {p.returncode}, stdout {p.stdout!r}, file bytes equal served bytes: {got == served}\n")


@lane("r6", 5, "r6-l5-blocked.txt")
def r6_l5():
    o = both_files(["video", "insert", "--file", "client_secret.json", "--categoryId", "22", "--privacy", "private", "--yes"], {"client_secret.json": b"{}"})
    ok = o["go"][:4] == o["head"][:4] and o["head"][0] == 1 and not o["head"][3] and "access denied: blocked file extension" in o["head"][2]
    return ok, f"go: exit {o['go'][0]} stderr {o['go'][2]!r}\nhead: exit {o['head'][0]} stderr {o['head'][2]!r}\nrequests sent by head: {len(o['head'][3])}\n"


@lane("r6", 6, "r6-l6-escape.txt")
def r6_l6():
    o = both_files(["video", "insert", "--file", "../outside.mp4", "--categoryId", "22", "--privacy", "private", "--yes"], {})
    (o["head"][4].parent / "outside.mp4").write_bytes(MP4)
    o = both_files(["video", "insert", "--file", "../outside.mp4", "--categoryId", "22", "--privacy", "private", "--yes"], {})
    ok = o["go"][:4] == o["head"][:4] and o["head"][0] == 1 and not o["head"][3]
    return ok, f"go: exit {o['go'][0]} stderr {o['go'][2]!r}\nhead: exit {o['head'][0]} stderr {o['head'][2]!r}\nrequests sent by head: {len(o['head'][3])}\n"


@lane("r6", 7, "r6-l7-watermark.txt")
def r6_l7():
    a = same_media(["watermark", "set", "--channelId", "c1", "--file", "w.png", "--yes"], {"w.png": PNG}, "SUBSTITUTED: recorder.")
    b = same_media(["watermark", "unset", "--channelId", "c1", "--yes"], {}, "")
    return a[0] and b[0], a[1] + b[1]


def big_upload(binary, size, rec, root):
    p = subprocess.run(["/usr/bin/time", "-l", binary, "video", "insert", "--file", "big.mp4", "--categoryId", "22", "--privacy", "private", "--yes"],
                       cwd=root, env=rec_env(rec), capture_output=True, text=True)
    m = __import__("re").search(r"(\d+)\s+maximum resident set size", p.stderr)
    return p.returncode, p.stdout, p.stderr.splitlines()[0] if p.stderr else "", int(m.group(1)) / 1e6 if m else -1


def sparse_file(size):
    root = Path(tempfile.mkdtemp())
    with open(root / "big.mp4", "wb") as f:
        f.write(MP4)
        f.truncate(size)
    return root


@lane("r6", 8, "r6-l8-resumable.txt")
def r6_l8():
    size = 2 << 30
    root = sparse_file(size)
    rec = Recorder(state=root / "state")
    code, out, err, rss = big_upload(HEAD, size, rec, root)
    chunks = [r.get("content_range") for r in rec.requests if r["path"] == "/resumable"]
    start = [r for r in rec.requests if r.get("upload") == "start"]
    rec.close()
    (root / "big.mp4").unlink()
    ok = code == 0 and len(start) == 1 and len(chunks) == 256 and chunks[0] == f"bytes 0-8388607/{size}" and chunks[-1] == f"bytes {size - 8388608}-{size - 1}/{size}"
    return ok, f"2 GiB sparse file; exit {code} {out.strip()} {err}\nsession starts: {len(start)}, chunks: {len(chunks)}, first {chunks[:1]}, last {chunks[-1:]}\npeak RSS {rss:.1f} MB\n"


@lane("r6", 9, "r6-l9-resume.txt")
def r6_l9():
    import signal
    import socket
    size = 200 << 20
    root = sparse_file(size)
    state = root / "state"
    state.mkdir()
    with socket.socket() as s_:
        s_.bind(("127.0.0.1", 0))
        port = s_.getsockname()[1]

    def start_recorder():
        p = subprocess.Popen([sys.executable, "-I", str(ROOT / "parity/recorder.py"), "--port", str(port), "--state", str(state)], stdout=subprocess.PIPE, text=True)
        p.stdout.readline()
        return p

    def received():
        f = state / "sessions.json"
        return list(json.loads(f.read_text()).values())[0]["received"] if f.exists() and f.read_text() else 0

    srv = start_recorder()
    upload = subprocess.Popen([HEAD, "video", "insert", "--file", "big.mp4", "--categoryId", "22", "--privacy", "private", "--yes"], cwd=root,
                              env={"PATH": "/usr/bin:/bin", "HOME": "/tmp", "YUTU_PARITY_ENDPOINT": f"http://127.0.0.1:{port}"}, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    while received() < 40 << 20:
        time.sleep(0.005)
    srv.send_signal(signal.SIGKILL)
    srv.wait()
    killed_at = received()
    time.sleep(2)
    srv = start_recorder()
    out, err = upload.communicate(timeout=300)
    srv.send_signal(signal.SIGKILL)
    srv.wait()
    log = [json.loads(l) for l in (state / "requests.jsonl").read_text().splitlines()]
    ranges = [r.get("content_range") for r in log if r["path"] == "/resumable"]
    status_queries = [i for i, c in enumerate(ranges) if c == f"bytes */{size}"]
    resumed = ranges[status_queries[0] + 1] if status_queries and status_queries[0] + 1 < len(ranges) else ""
    ok = upload.returncode == 0 and received() == size and bool(status_queries) and resumed.startswith(f"bytes {killed_at}-")
    return ok, (f"recorder SIGKILLed with {killed_at} bytes acknowledged, restarted on the same port 2 s later from its persisted session\n"
                f"chunk requests seen: {len(ranges)}; status query after restart: {[ranges[i] for i in status_queries]}; next chunk: {resumed}\n"
                f"final acknowledged bytes: {received()} of {size}\n" + fmt("yutu video insert (200 MiB)", upload.returncode, out, err))


@lane("r6", 10, "r6-l10-cleanup.txt")
def r6_l10():
    return sub_lane([["video", "delete", "--ids", "v1", "--yes"], ["video", "list", "--ids", "v1", "-o", "json"]])


# R7 ---------------------------------------------------------------------

def mcp_pair(fn, version="2025-06-18", elicit="accept", extra=None):
    from mcp_client import Stdio
    rec = Recorder()
    out = {}
    for name, binary in (("go", HARNESS), ("head", HEAD)):
        rec.reset()
        c = Stdio(binary, version, env=rec_env(rec, extra), elicit=elicit)
        out[name] = (fn(c), [differ.normalize_request(r) for r in rec.requests])
        c.close()
    rec.close()
    return out


@lane("r7", 1, "r7-l1-mcp-corpus.txt")
def r7_l1():
    ids = {c["id"] for c in json.loads((ROOT / "parity/corpus/mcp.json").read_text())}
    code, out, err = compare_ids(ids)
    return code == 0 and out.strip().endswith(", 0 differences"), f"MCP cases: {len(ids)} (registered E1, E5 and E6 fields excluded per parity/divergences.toml)\n" + out[-2000:] + err[-500:]


@lane("r7", 2, "r7-l2-claude.txt")
def r7_l2():
    o = mcp_pair(lambda c: c.request("tools/call", {"name": "playlist-list", "arguments": {"mine": True, "output": "table"}}))
    text = o["head"][0]["result"]["content"][0]["text"]
    ok = o["go"] == o["head"] and "TITLE" in text
    return ok, ("SUBSTITUTED: registering the binary in the operator's Claude Code config would change their setup and there is no test channel; an MCP client lists playlists over stdio against the recorder on both binaries.\n"
                f"identical result and requests: {o['go'] == o['head']}\n--- head tool text ---\n{text}\n")


@lane("r7", 3, "r7-l3-decline.txt")
def r7_l3():
    o = mcp_pair(lambda c: c.request("tools/call", {"name": "playlist-delete", "arguments": {"ids": ["p1"]}}), elicit="decline")
    ok = o["go"] == o["head"] and o["head"][1] == [] and o["head"][0]["result"]["isError"]
    return ok, f"declined elicitation; requests sent by head: {o['head'][1]}\nhead result: {json.dumps(o['head'][0])}\nidentical to Go: {o['go'] == o['head']}\n"


@lane("r7", 4, "r7-l4-resource.txt")
def r7_l4():
    o = mcp_pair(lambda c: c.request("resources/read", {"uri": "i18n://hl"}))
    return o["go"] == o["head"], f"identical payload and requests: {o['go'] == o['head']}\n{json.dumps(o['head'][0])[:800]}\n"


@lane("r7", 5, "r7-l5-template.txt")
def r7_l5():
    o = mcp_pair(lambda c: c.request("resources/read", {"uri": "video://category/ja"}))
    gq, hq = o["go"][1][0]["query"], o["head"][1][0]["query"]
    ok = "hl" not in gq and hq.get("hl") == ["ja"] and o["go"][0] == o["head"][0]
    return ok, f"Go query: {gq}\nhead query: {hq}\nDivergence E6: head honors {{hl}}; the payload served by the recorder is the same.\n"


@lane("r7", 6, "r7-l6-default.txt")
def r7_l6():
    o = mcp_pair(lambda c: c.request("tools/call", {"name": "video-list", "arguments": {}}))
    text = o["head"][0]["result"]["content"][0]["text"]
    ok = o["go"] == o["head"] and text.startswith('[{"') and "\n" not in text.rstrip("\n")
    return ok, f"identical: {o['go'] == o['head']}; compact JSON: {text[:160]}...\n"


@lane("r7", 7, "r7-l7-error.txt")
def r7_l7():
    o = mcp_pair(lambda c: c.request("tools/call", {"name": "video-list", "arguments": {"chart": "nope"}}))
    ok = o["go"] == o["head"] and o["head"][0]["result"].get("isError")
    return ok, f"identical: {o['go'] == o['head']}\nhead: {json.dumps(o['head'][0])}\n"


@lane("r7", 8, "r7-l8-quiet.txt")
def r7_l8():
    msgs = [{"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "l", "version": "0"}}},
            {"jsonrpc": "2.0", "method": "notifications/initialized"}, {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}]
    rec = Recorder()
    errs = {}
    for name, binary in (("go", HARNESS), ("head", HEAD)):
        p = subprocess.run([binary, "mcp"], input="".join(json.dumps(m) + "\n" for m in msgs), env=rec_env(rec, {"YUTU_LOG_LEVEL": "INFO"}), capture_output=True, text=True, timeout=30)
        errs[name] = p.stderr
    rec.close()
    ok = '"jsonrpc"' not in errs["head"] and '"jsonrpc"' in errs["go"]
    return ok, f"Divergence E5. Go stderr at INFO carries the frames: {'\"jsonrpc\"' in errs['go']}; head: {'\"jsonrpc\"' in errs['head']}\n--- head stderr ---\n{errs['head']}\n--- go stderr (first 400 bytes) ---\n{errs['go'][:400]}\n"


@lane("r7", 9, "r7-l9-hints.txt")
def r7_l9():
    o = mcp_pair(lambda c: c.request("tools/list"))
    hints = {k: {t["name"]: t.get("annotations") for t in v[0]["result"]["tools"]} for k, v in o.items()}
    ok = hints["go"] == hints["head"] and len(hints["head"]) == 74
    counts = {k: sum(1 for a in hints["head"].values() if a.get(k)) for k in ("readOnlyHint", "destructiveHint", "idempotentHint", "openWorldHint")}
    return ok, f"tools: {len(hints['head'])}; annotations identical to Go for every tool: {hints['go'] == hints['head']}\ncounts of true hints: {counts}\n"


@lane("r7", 10, "r7-l10-soak.txt")
def r7_l10():
    import resource
    from mcp_client import Stdio
    rec = Recorder()
    c = Stdio(HEAD, env=rec_env(rec))
    fails = 0
    for i in range(200):
        r = c.request("tools/call", {"name": "video-list", "arguments": {"ids": [f"v{i}"]}})
        fails += bool(r.get("error") or r.get("result", {}).get("isError"))
    rss = sh(["ps", "-o", "rss=", "-p", str(c.p.pid)])[1].strip()
    c.close()
    rec.close()
    mb = int(rss) / 1024 if rss.isdigit() else -1
    return fails == 0 and 0 < mb < 20, f"200 sequential tools/call in one session: {200 - fails} succeeded; server RSS after the run: {mb:.1f} MB\n"


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
