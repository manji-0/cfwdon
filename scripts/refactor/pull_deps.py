#!/usr/bin/env python3
"""Pull missing private helpers into <dst> from candidate source files until <dst> resolves.

usage: pull_deps.py <repo_root> <dst.rs (relative to worker src)> <src.rs>...
"""
import json
import os
import re
import subprocess
import sys

root = sys.argv[1]
src_dir = os.path.join(root, "crates/cfwdon-worker/src")
dst = sys.argv[2]
sources = sys.argv[3:]
here = os.path.dirname(os.path.abspath(__file__))

DEF = r"^(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:fn|struct|enum|type|trait|const|static)\s+{}\b"

for it in range(15):
    out = subprocess.run(
        ["devbox", "run", "--", "cargo", "check", "-p", "cfwdon-worker", "--all-targets", "--message-format=json"],
        cwd=root, capture_output=True, text=True,
    ).stdout
    missing = set()
    for line in out.splitlines():
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get("reason") != "compiler-message":
            continue
        m = m["message"]
        if m["level"] != "error":
            continue
        sp = (m.get("spans") or [{}])[0]
        if not sp.get("file_name", "").endswith("/" + dst):
            continue
        mm = re.search(r"cannot find (?:\w+ )+`(\w+)`", m["message"])
        if mm:
            missing.add(mm.group(1))
    todo = {}
    for name in missing:
        for s in sources:
            text = open(os.path.join(src_dir, s)).read()
            if re.search(DEF.format(re.escape(name)), text, re.M):
                todo.setdefault(s, []).append(name)
                break
    print(f"iter {it}: missing {sorted(missing)} -> pulling {todo}", flush=True)
    if not todo:
        break
    for s, names in todo.items():
        subprocess.run(
            ["python3", os.path.join(here, "move_items.py"), s, dst, "--pub", *names],
            cwd=src_dir, check=True,
        )
