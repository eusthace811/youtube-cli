"""Runs command lines that exercise the cobra/pflag engine against the Go
oracle and a Rust binary, and prints every difference.

    python3 -I parity/engine_diff.py --go ref/yutu-go --bin target/release/youtube

The cases in engine-cases.json cover parsing errors, help routing,
suggestions, `__complete` and the version banner's terminal heuristics.
They avoid commands whose leaf operation runs, which R4 onward compare
through run.py.
"""
import argparse
import json
import subprocess
import sys
from pathlib import Path

CASES = Path(__file__).resolve().parent / "engine-cases.json"


def run(binary, case):
    env = {"PATH": "/usr/bin:/bin", "HOME": "/tmp"}
    env.update(case.get("env", {}))
    p = subprocess.run([binary, *case["argv"]], capture_output=True, env=env,
                       stdin=subprocess.DEVNULL, timeout=20)
    return {"stdout": p.stdout, "stderr": p.stderr, "exit": p.returncode}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--go", required=True)
    ap.add_argument("--bin", required=True)
    args = ap.parse_args()
    cases = json.loads(CASES.read_text())
    differences = 0
    for case in cases:
        want, got = run(args.go, case), run(args.bin, case)
        for key in want:
            if want[key] != got[key]:
                differences += 1
                print(f"{json.dumps(case, ensure_ascii=False)}: {key}: {want[key]!r:.300} != {got[key]!r:.300}")
    print(f"engine: {len(cases)} cases, {differences} differences")
    sys.exit(1 if differences else 0)


if __name__ == "__main__":
    main()
