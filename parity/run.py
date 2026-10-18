"""Runs parity cases against one binary under the recorder.

    python3 -I parity/run.py --bin ref/yutu-go-harness --all
    python3 -I parity/run.py --bin target/release/yutu --only version,help --compare

Without --compare the results become the golden set. With --compare they are
diffed against parity/golden/cases and only differences are reported.
"""
import argparse
import base64
import json
import os
import pty
import select
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from recorder import Recorder  # noqa: E402
import diff as differ  # noqa: E402

PARITY = Path(__file__).resolve().parent
CORPUS = PARITY / "corpus"
GOLDEN = PARITY / "golden" / "cases"
WORK = Path("/tmp/yutu-parity")


def load_cases(only=None, effects=None, no_media=False):
    cases = []
    for f in sorted(CORPUS.glob("*.json")):
        cases.extend(json.loads(f.read_text()))
    if only:
        wanted = set(only)
        cases = [c for c in cases if c["id"] in wanted or c.get("group") in wanted]
    if effects:
        cases = [c for c in cases if c.get("effect") in effects]
    if no_media:
        cases = [c for c in cases if not c.get("media")]
    return cases


def prepare_root(case):
    root = WORK / "root"
    shutil.rmtree(WORK, ignore_errors=True)
    root.mkdir(parents=True)
    for name, content in case.get("files", {}).items():
        p = root / name
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(base64.b64decode(content["b64"]) if isinstance(content, dict) else content.encode())
    return root


def case_env(case, root, endpoint):
    env = {"PATH": "/usr/bin:/bin", "HOME": str(root), "LANG": "C.UTF-8",
           "YUTU_PARITY_ENDPOINT": endpoint}
    env.update(case.get("env", {}))
    return {k: v for k, v in env.items() if v is not None}


def run_tty(argv, env, cwd, stdin_text, timeout):
    """Runs with stdin, stdout and stderr on one pseudo-terminal."""
    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(cwd)
        os.execve(argv[0], argv, env)
    out = b""
    sent = stdin_text is None
    deadline = time.time() + timeout
    while time.time() < deadline:
        r, _, _ = select.select([fd], [], [], 0.05)
        if r:
            try:
                chunk = os.read(fd, 65536)
            except OSError:
                break
            if not chunk:
                break
            out += chunk
        elif not sent:
            os.write(fd, stdin_text.encode())
            sent = True
    _, status = os.waitpid(pid, 0)
    return out.decode("utf-8", "replace"), "", os.waitstatus_to_exitcode(status)


class LineReader:
    """Reads lines from a raw fd with a timeout, without stdio buffering hiding data from select."""

    def __init__(self, fd):
        self.fd, self.buf = fd, b""

    def readline(self, timeout):
        deadline = time.time() + timeout
        while b"\n" not in self.buf:
            left = deadline - time.time()
            if left <= 0 or not select.select([self.fd], [], [], left)[0]:
                return ""
            chunk = os.read(self.fd, 1 << 20)
            if not chunk:
                return ""
            self.buf += chunk
        line, _, self.buf = self.buf.partition(b"\n")
        return line.decode("utf-8", "replace") + "\n"


def mcp_session(argv, env, cwd, messages, timeout, elicit="accept"):
    """Speaks JSON-RPC over stdio. Server-to-client requests such as
    elicitation/create are recorded and answered with the case's `elicit` action."""
    errfile = tempfile.TemporaryFile()
    p = subprocess.Popen(argv + ["mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=errfile, env=env, cwd=cwd, text=True)
    reader = LineReader(p.stdout.fileno())
    replies = []
    try:
        for msg in messages:
            p.stdin.write(json.dumps(msg) + "\n")
            p.stdin.flush()
            if "id" not in msg:
                continue
            while True:
                line = reader.readline(timeout)
                if not line:
                    replies.append(json.dumps({"parity": "no reply", "to": msg["id"]}) + "\n")
                    break
                frame = json.loads(line)
                if "method" in frame and "id" in frame:
                    replies.append(line)
                    answer = {"action": elicit} if elicit != "error" else None
                    reply = {"jsonrpc": "2.0", "id": frame["id"], "result": answer} if answer else \
                        {"jsonrpc": "2.0", "id": frame["id"], "error": {"code": -32601, "message": "elicitation not supported"}}
                    p.stdin.write(json.dumps(reply) + "\n")
                    p.stdin.flush()
                    continue
                if frame.get("id") == msg["id"]:
                    replies.append(line)
                    break
    finally:
        p.stdin.close()
        try:
            p.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            p.kill()
            p.wait()
    errfile.seek(0)
    err = errfile.read().decode("utf-8", "replace")
    return "".join(replies), err, 0


def run_case(binary, case, rec, timeout=30):
    rec.reset(fail=case.get("fail"), overrides=case.get("fixtures"))
    root = prepare_root(case)
    env = case_env(case, root, rec.url)
    argv = [str(Path(binary).resolve())] + case.get("argv", [])
    if case.get("surface") == "mcp":
        stdout, stderr, code = mcp_session(argv, env, root, case["messages"], timeout, case.get("elicit", "accept"))
    elif case.get("tty"):
        stdout, stderr, code = run_tty(argv, env, root, case.get("stdin"), timeout)
    else:
        p = subprocess.run(argv, input=case.get("stdin", ""), capture_output=True, env=env,
                           cwd=root, text=True, timeout=timeout)
        stdout, stderr, code = p.stdout, p.stderr, p.returncode
    files = {}
    for name in case.get("collect", []):
        p = root / name
        files[name] = p.read_text() if p.exists() else None
    return {"id": case["id"], "requests": list(rec.requests), "stdout": stdout,
            "stderr": stderr, "exit": code, "files": files}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", required=True)
    ap.add_argument("--all", action="store_true")
    ap.add_argument("--only", default="")
    ap.add_argument("--effect", default="")
    ap.add_argument("--no-media", action="store_true")
    ap.add_argument("--compare", action="store_true")
    ap.add_argument("--out", default=str(GOLDEN))
    ap.add_argument("--fixtures", default=None, help="serve fixtures from this directory instead")
    ap.add_argument("--allow", default="", help="comma separated Appendix E ids accepted as differences")
    args = ap.parse_args()
    cases = load_cases(args.only.split(",") if args.only else None,
                       set(args.effect.split(",")) if args.effect else None, args.no_media)
    rec = Recorder(args.fixtures) if args.fixtures else Recorder()
    out = Path(args.out)
    if not args.compare:
        out.mkdir(parents=True, exist_ok=True)
    allowed = set(filter(None, args.allow.split(",")))
    total = 0
    differences = 0
    for case in cases:
        result = run_case(args.bin, case, rec)
        total += 1
        if args.compare:
            golden = json.loads((GOLDEN / f"{case['id']}.json").read_text())
            found = differ.compare(golden, result, case, allowed)
            for d in found:
                print(f"{case['id']}: {d}")
            differences += len(found)
        else:
            (out / f"{case['id']}.json").write_text(json.dumps(result, ensure_ascii=False, indent=1, sort_keys=True) + "\n")
    rec.close()
    if args.compare:
        print(f"cases: {total}, {differences} differences")
        sys.exit(1 if differences else 0)
    print(f"corpus: {total} cases, golden written")


if __name__ == "__main__":
    main()
