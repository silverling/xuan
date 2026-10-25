#!/usr/bin/env python3
"""Build Xuan's opt-in adapter and replay its size-dialog scenarios."""
import argparse
import json
from pathlib import Path
import subprocess
import sys

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("runner", type=Path, help="pinned interaction_check.py")
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    build = subprocess.run(
        ["cargo", "build", "--locked", "--features", "interaction-probe",
         "--bin", "xuan", "--message-format=json"],
        cwd=root, check=True, stdout=subprocess.PIPE, text=True,
    )
    executable = None
    for line in build.stdout.splitlines():
        message = json.loads(line)
        if message.get("reason") == "compiler-artifact" and message.get("target", {}).get("name") == "xuan":
            executable = message.get("executable") or executable
    if executable is None:
        raise RuntimeError("cargo did not report the Xuan executable")
    scenarios = sorted((root / "tests/interactions/size-dialog").glob("*.json"))
    if not scenarios:
        raise RuntimeError("no interaction scenarios found")
    subprocess.run(
        [sys.executable, str(args.runner.resolve()), "--report", str(args.report.resolve()),
         *map(str, scenarios), "--", executable, "--interaction-probe"],
        cwd=root, check=True,
    )

if __name__ == "__main__":
    main()

