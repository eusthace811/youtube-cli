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
