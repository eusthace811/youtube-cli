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


def recorder():
    sys.path.insert(0, str(ROOT / "parity"))
    from recorder import Recorder
    return Recorder()


def alternate(cmds, n, env_for, cwd=None):
    """Runs each argv in turn, n rounds, returning per-command wall times."""
    times = {k: [] for k in cmds}
    for _ in range(3):
        for k, argv in cmds.items():
            subprocess.run(argv, env=env_for(k), cwd=cwd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(n):
        for k, argv in cmds.items():
            t = time.perf_counter()
            subprocess.run(argv, env=env_for(k), cwd=cwd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            times[k].append((time.perf_counter() - t) * 1000)
    return times


@probe("r3")
def r3():
    import tempfile
    sys.path.insert(0, str(ROOT / "parity"))
    from mockoauth import MockOAuth
    root = Path(tempfile.mkdtemp())
    oauth, rec = MockOAuth(), recorder()
    (root / "client_secret.json").write_text(oauth.client_secret())
    subprocess.run([str(ROOT / "ref/yutu-go-tokenwrite"), str(root / "youtube.token.json"), "cached", "r", "2030-01-01T00:00:00Z"], check=True)
    base = {"PATH": "/usr/bin:/bin", "HOME": str(root), "YUTU_PARITY_ENDPOINT": rec.url}
    envs = {"go": base, "head": {**base, "YUTU_PARITY_AUTH": "1"}}
    argv = ["channel", "list", "--for", "mine", "-o", "json"]
    t = alternate({"go": [HARNESS] + argv, "head": [HEAD] + argv}, 60, envs.get, cwd=root)
    gm, hm = statistics.median(t["go"]), statistics.median(t["head"])
    auth_seen = sum(1 for r in rec.requests if r.get("authorization") == "Bearer cached")
    ok = hm <= 0.5 * gm and auth_seen >= 60
    text = (f"Metric: wall time of `channel list --for mine -o json` with a valid cached token against the recorder (60 interleaved runs).\n"
            f"Trunk is the Go harness, which bypasses auth; head loads and uses the token file (YUTU_PARITY_AUTH=1): {auth_seen} head requests carried the cached bearer.\n"
            f"Trunk median {gm:.2f} ms, head median {hm:.2f} ms ({hm / gm:.2f}x).\nRule: head <= 0.5x trunk.\n")
    rec.close()
    oauth.close()
    return ok, text


def rss_of(argv, env, cwd=None):
    t = time.perf_counter()
    p = subprocess.run(["/usr/bin/time", "-l"] + argv, env=env, cwd=cwd, capture_output=True, text=True)
    wall = time.perf_counter() - t
    m = re.search(r"(\d+)\s+maximum resident set size", p.stderr)
    return p.returncode, wall, (int(m.group(1)) / 1e6 if m else float("nan"))


@probe("r4")
def r4():
    rec = recorder()
    item = json.loads((ROOT / "parity/fixtures/GET_videos.json").read_text())["pages"][0]["items"][0]
    pages = []
    for i in range(200):
        page = {"kind": "youtube#videoListResponse", "items": [dict(item, id=f"v{i}-{j}") for j in range(20)]}
        if i < 199:
            page["nextPageToken"] = f"t{i + 1}"
        page["_token"] = "" if i == 0 else f"t{i}"
        pages.append(page)
    rec.reset(overrides={"GET_videos": {"pages": pages}})
    env = {"PATH": "/usr/bin:/bin", "HOME": "/tmp", "YUTU_PARITY_ENDPOINT": rec.url}
    argv = ["video", "list", "--maxResults", "0", "-o", "json"]
    def reset():
        rec.reset(overrides={"GET_videos": {"pages": pages}})

    walls = {"go": [], "head": [], "floor": []}
    for i in range(13):
        for name, argv_ in (("go", [HARNESS] + argv), ("head", [HEAD] + argv), ("floor", ["/usr/bin/true"])):
            reset()
            t = time.perf_counter()
            subprocess.run(argv_, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            if i >= 3:
                walls[name].append((time.perf_counter() - t) * 1000)
    rss = {}
    for name, binary in (("go", HARNESS), ("head", HEAD)):
        vals = []
        for _ in range(3):
            reset()
            vals.append(rss_of([binary] + argv, env)[2])
        rss[name] = statistics.median(vals)
    rec.close()
    gw, hw, fw = statistics.median(walls["go"]), statistics.median(walls["head"]), statistics.median(walls["floor"])
    gr, hr = rss["go"], rss["head"]
    ok = hw <= 0.6 * gw and hr <= 0.4 * gr
    return ok, (f"Metric: `video list --maxResults 0 -o json` over 200 pages of 20 items (34.7 MB) from the recorder; wall time from 10 interleaved runs\n"
                f"after 3 warmups, timed directly (no /usr/bin/time wrapper, whose fixed cost would compress the ratio); peak RSS from 3 separate /usr/bin/time -l runs.\n"
                f"Spawn floor /usr/bin/true: {fw:.1f} ms. The recorder (Python, same machine) serves 34.7 MB per run in about 90 ms, a floor shared by both.\n"
                f"Trunk (Go harness): median {gw:.1f} ms, peak RSS {gr:.1f} MB\nHead: median {hw:.1f} ms ({hw / gw:.2f}x), peak RSS {hr:.1f} MB ({hr / gr:.2f}x)\n"
                f"Rule: head <= 0.6x trunk wall time and <= 0.4x trunk RSS.\n")


@probe("r5")
def r5():
    rec = recorder()
    env = {"PATH": "/usr/bin:/bin", "HOME": "/tmp", "YUTU_PARITY_ENDPOINT": rec.url}
    argv = ["playlistItem", "insert", "--kind", "video", "--kVideoId", "v1", "--playlistId", "p1", "--channelId", "c1", "--yes"]
    batches = {"go": [], "head": []}
    for _ in range(5):
        for name, binary in (("go", HARNESS), ("head", HEAD)):
            t = time.perf_counter()
            for _ in range(100):
                subprocess.run([binary] + argv, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            batches[name].append(time.perf_counter() - t)
    rec.close()
    g, h = statistics.median(batches["go"]), statistics.median(batches["head"])
    return h <= 0.5 * g, (f"Metric: 100 sequential `playlistItem insert --yes` invocations against the recorder, five alternating batches.\n"
                          f"Trunk batches (s): {', '.join(f'{x:.2f}' for x in batches['go'])} -> median {g:.2f} s\n"
                          f"Head batches (s):  {', '.join(f'{x:.2f}' for x in batches['head'])} -> median {h:.2f} s ({h / g:.2f}x)\nRule: head <= 0.5x trunk.\n")


@probe("r6")
def r6():
    import tempfile
    size = 2 << 30
    root = Path(tempfile.mkdtemp())
    with open(root / "big.mp4", "wb") as f:
        f.write(bytes.fromhex("000000186674797069736f6d0000020069736f6d69736f32"))
        f.truncate(size)
    argv = ["video", "insert", "--file", "big.mp4", "--categoryId", "22", "--privacy", "private", "--yes"]
    runs = {"go": [], "head": []}
    for _ in range(3):
        for name, binary in (("go", HARNESS), ("head", HEAD)):
            rec = recorder()
            env = {"PATH": "/usr/bin:/bin", "HOME": "/tmp", "YUTU_PARITY_ENDPOINT": rec.url}
            runs[name].append(rss_of([binary] + argv, env, cwd=root))
            rec.close()
    (root / "big.mp4").unlink()
    tp = {k: statistics.median(size / x[1] / 1e6 for x in v) for k, v in runs.items()}
    rss = {k: statistics.median(x[2] for x in v) for k, v in runs.items()}
    ok = all(x[0] == 0 for v in runs.values() for x in v) and rss["head"] <= 32 and tp["head"] >= tp["go"]
    return ok, (f"Metric: peak RSS and throughput uploading a 2 GiB file to the recorder (resumable; Go uses 16 MiB chunks, head 8 MiB per E7), 3 alternating runs.\n"
                f"Trunk: {tp['go']:.0f} MB/s, peak RSS {rss['go']:.1f} MB\nHead:  {tp['head']:.0f} MB/s, peak RSS {rss['head']:.1f} MB\n"
                f"Rule: head RSS <= 32 MB and head throughput >= trunk.\nNote: the recorder runs in Python on the same machine and is part of both measurements.\n")


@probe("r7")
def r7():
    import select

    def session(binary):
        p = subprocess.Popen([binary, "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=ENV)
        fd = p.stdout.fileno()
        buf = b""

        def call(i, method, params):
            nonlocal buf
            p.stdin.write((json.dumps({"jsonrpc": "2.0", "id": i, "method": method, "params": params}) + "\n").encode())
            p.stdin.flush()
            t = time.perf_counter()
            while b"\n" not in buf:
                select.select([fd], [], [], 5)
                buf += os.read(fd, 1 << 20)
            _, _, buf = buf.partition(b"\n")
            return (time.perf_counter() - t) * 1000

        t0 = time.perf_counter()
        call(1, "initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "perf", "version": "0"}})
        init = (time.perf_counter() - t0) * 1000
        p.stdin.write(b'{"jsonrpc":"2.0","method":"notifications/initialized"}\n')
        p.stdin.flush()
        tl = call(2, "tools/list", {})
        p.kill()
        p.wait()
        return init, tl

    res = {"go": [], "head": []}
    for _ in range(10):
        for name, binary in (("go", GO), ("head", HEAD)):
            res[name].append(session(binary))
    gi, gl = statistics.median(x[0] for x in res["go"]), statistics.median(x[1] for x in res["go"])
    hi, hl = statistics.median(x[0] for x in res["head"]), statistics.median(x[1] for x in res["head"])
    ok = hi <= 4.0 and hl <= gl
    return ok, (f"Metric: initialize (from spawn to reply) and tools/list latency over 10 fresh stdio sessions, alternating.\n"
                f"Trunk: initialize {gi:.2f} ms, tools/list {gl:.2f} ms\nHead:  initialize {hi:.2f} ms, tools/list {hl:.2f} ms\n"
                f"Rule: head initialize <= 4 ms and tools/list <= trunk.\n")


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
