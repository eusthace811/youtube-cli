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
