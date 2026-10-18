"""Performance probes per PR, alternating trunk (Go) and head (Rust) in one
loop with a spawn floor, per the benchmark checklist. Each probe prints the
medians and writes docs/evidence/<pr>/<pr>-perf.txt with a PASS or FAIL
verdict against the plan's rule.

    python3 -I parity/perf.py r1
"""
import json
import os
import re
import statistics
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
GO = str(ROOT / "ref" / "yutu-go")
HARNESS = str(ROOT / "ref" / "yutu-go-harness")
HEAD = str(ROOT / "target" / "release" / "yutu")
ENV = {"PATH": "/usr/bin:/bin", "HOME": "/tmp"}
PROBES = {}


def probe(pr):
    def wrap(fn):
        PROBES[pr] = fn
        return fn
    return wrap


def wall(argv, env=ENV, stdin=None):
    t = time.perf_counter()
    subprocess.run(argv, env=env, stdin=stdin, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return (time.perf_counter() - t) * 1000


def rss_mb(argv, env=ENV):
    p = subprocess.run(["/usr/bin/time", "-l"] + argv, env=env, capture_output=True, text=True)
    m = re.search(r"(\d+)\s+maximum resident set size", p.stderr)
    return int(m.group(1)) / 1e6 if m else float("nan")


def stats(xs):
    xs = sorted(xs)
    return statistics.median(xs), xs[int(len(xs) * 0.9) - 1]


@probe("r1")
def r1():
    for _ in range(5):
        wall([GO, "version"]), wall([HEAD, "version"]), wall(["/usr/bin/true"])
    go, head, floor = [], [], []
    for _ in range(60):
        go.append(wall([GO, "version"]))
        head.append(wall([HEAD, "version"]))
        floor.append(wall(["/usr/bin/true"]))
    (gm, gp), (hm, hp), (fm, fp) = stats(go), stats(head), stats(floor)
    g_rss, h_rss = rss_mb([GO, "version"]), rss_mb([HEAD, "version"])
    g_size, h_size = os.path.getsize(GO) / 1e6, os.path.getsize(HEAD) / 1e6
    ok = hm <= 4.0 and h_rss <= 12 and h_size <= 15
    text = (f"Metric: `yutu version` wall time over 60 interleaved runs, peak RSS, stripped binary size.\n"
            f"Spawn floor /usr/bin/true: median {fm:.2f} ms, p90 {fp:.2f} ms\n"
            f"Trunk (Go):  median {gm:.2f} ms, p90 {gp:.2f} ms, RSS {g_rss:.1f} MB, binary {g_size:.1f} MB\n"
            f"Head (Rust): median {hm:.2f} ms, p90 {hp:.2f} ms, RSS {h_rss:.1f} MB, binary {h_size:.1f} MB\n"
            f"Rule: head median <= 4.0 ms, RSS <= 12 MB, binary <= 15 MB.\n")
    return ok, text


@probe("r2")
def r2():
    import tempfile
    d = Path(tempfile.mkdtemp())
    big = d / "GET_videos.json"
    subprocess.run([sys.executable, "-I", str(ROOT / "parity/gen_fixtures.py"), "--big", str(big)], check=True)
    env = {**os.environ, "BIG_FIXTURE": str(big), "GOPATH": str(ROOT / ".tools/gopath"), "GOTOOLCHAIN": "local", "GOFLAGS": "-mod=mod"}
    go_bin = str(ROOT / ".tools/go/bin/go")
    subprocess.run(["cargo", "build", "-q", "--release", "-p", "yutu-api", "--bench", "decode"], cwd=ROOT, check=True, env=env)
    go_ms, rust_ms = [], []
    for _ in range(5):
        out = subprocess.run([go_bin, "test", "-run", "x", "-bench", ".", "-benchtime", "20x"], cwd=ROOT / "ref/harness/bench", env=env, capture_output=True, text=True).stdout
        go_ms.append(int(re.search(r"(\d+) ns/op", out).group(1)) / 1e6)
        out = subprocess.run(["cargo", "bench", "-q", "-p", "yutu-api", "--bench", "decode"], cwd=ROOT, env=env, capture_output=True, text=True).stdout
        rust_ms.append(float(re.search(r"median ([\d.]+) ms", out).group(1)))
    gm, rm = statistics.median(go_ms), statistics.median(rust_ms)
    size = os.path.getsize(HEAD) / 1e6
    ok = rm <= gm and size <= 15
    text = (f"Metric: decode a 500-video VideoListResponse ({big.stat().st_size} bytes) and re-encode its items as JSON.\n"
            f"Probe: Go `go test -bench` (20 iterations, ns/op) and Rust `cargo bench --bench decode` (median of 20), alternated 5 times.\n"
            f"Go runs (ms):   {', '.join(f'{x:.2f}' for x in go_ms)} -> median {gm:.2f} ms\n"
            f"Rust runs (ms): {', '.join(f'{x:.2f}' for x in rust_ms)} -> median {rm:.2f} ms\n"
            f"Binary: target/release/yutu {size:.2f} MB (yutu-api not yet linked into the CLI at this PR).\n"
            f"Rule: head median <= Go median and binary <= 15 MB.\n")
    return ok, text


def loadgen(url, body, seconds):
    """crates/yutu-mcp/examples/loadgen: 50 keep-alive HTTP/1.1 clients."""
    gen = os.environ.get("YUTU_LOADGEN", str(ROOT / "target" / "release" / "examples" / "loadgen"))
    out = subprocess.run([gen, url, str(seconds), "50", body], capture_output=True, text=True, check=True).stdout
    r = json.loads(out)
    return r["rps"], r["p50_ms"], r["done"], r["failed"]


def free_port():
    import socket
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


# TIME_WAIT lasts 2 x 15 s MSL on macOS; waiting it out frees every port the
# previous run used.
R8_COOLDOWN = 35
R8_SECONDS = 3


def wait_port(port):
    import socket
    for _ in range(200):
        try:
            socket.create_connection(("127.0.0.1", port), timeout=0.1).close()
            return
        except OSError:
            time.sleep(0.05)
    raise RuntimeError(f"nothing listening on {port}")


@probe("r8")
def r8():
    import tempfile
    import urllib.request
    head = os.environ.get("YUTU_HEAD", HEAD)
    mock = os.environ.get("YUTU_PERFMOCK", str(ROOT / "target" / "release" / "examples" / "perfmock"))
    work = Path(tempfile.mkdtemp())
    body = work / "body.json"
    body.write_text(json.dumps({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                                "params": {"name": "video-list", "arguments": {"ids": ["v1"]}}}))
    mock_port = free_port()
    procs = [subprocess.Popen([mock, str(mock_port), str(ROOT / "parity/fixtures/GET_videos.json")])]
    upstream = f"http://127.0.0.1:{mock_port}"
    urls = {}
    try:
        wait_port(mock_port)
        for name, binary in (("go", HARNESS), ("head", head)):
            port = free_port()
            env = {**ENV, "YUTU_PARITY_ENDPOINT": upstream, "YUTU_PARITY_TOKENINFO": f"{upstream}/tokeninfo"}
            procs.append(subprocess.Popen([binary, "mcp", "--mode", "http", "--port", str(port)], env=env, cwd=work,
                                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
            wait_port(port)
            urls[name] = f"http://127.0.0.1:{port}/mcp"
            req = urllib.request.Request(urls[name], data=body.read_bytes(), method="POST", headers={
                "Content-Type": "application/json", "Accept": "application/json, text/event-stream",
                "Authorization": "Bearer good-token", "Mcp-Protocol-Version": "2025-06-18"})
            reply = json.loads(urllib.request.urlopen(req, timeout=10).read())
            text = reply["result"]["content"][0]["text"]
            assert not reply["result"].get("isError") and text.startswith('[{"ageGating"'), reply
        rounds = []
        for _ in range(3):
            r = {}
            for name in ("go", "head"):
                time.sleep(R8_COOLDOWN)
                r[name] = loadgen(urls[name], str(body), R8_SECONDS)
            rounds.append(r)
    finally:
        for p in procs:
            p.terminate()
            p.wait()
    med = lambda name, i: statistics.median(r[name][i] for r in rounds)  # noqa: E731
    g_rps, h_rps, g_p50, h_p50 = med("go", 0), med("head", 0), med("go", 1), med("head", 1)
    failed = sum(r[n][3] for r in rounds for n in r)
    ok = h_rps >= 2 * g_rps and h_p50 <= g_p50 and failed == 0
    lines = [f"Round {i + 1}: " + "; ".join(
        f"{label} {r[n][0]:.0f} req/s p50 {r[n][1]:.2f} ms ({r[n][2]} done, {r[n][3]} failed)" for n, label in (("go", "Go"), ("head", "head"))) for i, r in enumerate(rounds)]
    text = ("Metric: requests/second and p50 latency for `tools/call video-list` (ids=[v1]) over stateless HTTP,\n"
            f"50 keep-alive HTTP/1.1 clients (crates/yutu-mcp/examples/loadgen, {R8_SECONDS} s per run), Go harness and head\n"
            "alternated, three rounds. `ab` was dropped: it speaks HTTP/1.0, and Go closes the connection after\n"
            "every chunked reply, so `ab` would time Go's TCP handshakes.\n"
            "Rounds are 3 s, not the plan's 30 s: Go's http.DefaultClient keeps 2 idle connections per host, so it\n"
            "opens and closes upstream connections on most calls and exhausts macOS's 16,384 ephemeral ports\n"
            "(TIME_WAIT lasts 30 s) within about 4 s, after which its calls fail with EADDRNOTAVAIL. Each run\n"
            f"starts after a {R8_COOLDOWN} s cooldown. A reply counts as failed unless it is HTTP 200 without\n"
            "\"isError\":true, and the first reply of each server is checked to be a successful video list.\n"
            f"Upstream: crates/yutu-mcp/examples/perfmock answers tokeninfo and videos.list ({len(open(ROOT / 'parity/fixtures/GET_videos.json').read())} byte fixture,\n"
            "first page) from memory; parity/recorder.py tops out near 2,100 req/s and would cap both servers.\n"
            "Go calls tokeninfo on every request; head verifies the token once and caches it (E4).\n"
            f"Machine: {os.cpu_count()} logical CPUs, default YUTU_LOG_LEVEL (one INFO line per call on both).\n"
            + "\n".join(lines) + "\n"
            f"Median: Go {g_rps:.0f} req/s p50 {g_p50:.2f} ms; head {h_rps:.0f} req/s p50 {h_p50:.2f} ms "
            f"({h_rps / g_rps:.2f}x throughput).\n"
            "Rule: head throughput >= 2x Go and head p50 <= Go p50, with no failed requests.\n")
    return ok, text


def main():
    pr = sys.argv[1]
    ok, text = PROBES[pr]()
    verdict = "PASS" if ok else "FAIL"
    out = ROOT / "docs" / "evidence" / pr
    out.mkdir(parents=True, exist_ok=True)
    (out / f"{pr}-perf.txt").write_text(f"{pr} perf: {verdict}\n\n{text}")
    print(text + f"{pr} perf: {verdict}")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
