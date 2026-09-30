#!/usr/bin/env python3
"""Strip E0432-unresolved names out of `use` lists.

Names defined inside the worker crate are dropped (the follow-up fix_imports pass
re-imports them via rustc suggestions). Names that used to come from the crate
root's external imports are re-imported explicitly from their real crate.
"""
import collections
import glob
import json
import os
import re
import subprocess
import sys

root = sys.argv[1]
src = os.path.join(root, "crates/cfwdon-worker/src")
target = sys.argv[2] if len(sys.argv) > 2 else None

ROOT_EXPLICIT = {
    "AppConfig": "cfwdon_core",
    "InstanceCapabilities": "cfwdon_domain",
    "InstanceSummary": "cfwdon_domain",
    "LocalAccount": "cfwdon_domain",
    "ProfileField": "cfwdon_domain",
    "SoftwareInfo": "cfwdon_domain",
    "StatusDraft": "cfwdon_domain",
    "Visibility": "cfwdon_domain",
    "Serialize": "serde",
    "LocalStatus": "cfwdon_domain",
    "LocalStatusRecord": "cfwdon_domain",
    "RemoteStatus": "cfwdon_domain",
    "LocalAccountRecord": "cfwdon_domain",
    "RemoteStatusRecord": "cfwdon_domain",
}

defs = set()
DEF_RE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?"
    r"(?:fn|struct|enum|type|trait|const|static|mod|union)\s+([A-Za-z_][A-Za-z0-9_]*)",
    re.M,
)
for p in glob.glob(os.path.join(src, "**/*.rs"), recursive=True):
    s = open(p).read()
    defs.update(DEF_RE.findall(s))
    defs.update(re.findall(r"macro_rules!\s+([A-Za-z_][A-Za-z0-9_]*)", s))

cmd = ["devbox", "run", "--", "cargo", "check", "-p", "cfwdon-worker", "--message-format=json"]
cmd += ["--target", target, "--lib"] if target else ["--all-targets"]
out = subprocess.run(cmd, cwd=root, capture_output=True, text=True).stdout

removals = collections.defaultdict(set)  # file -> {(byte_start, byte_end, name)}
for line in out.splitlines():
    try:
        m = json.loads(line)
    except ValueError:
        continue
    if m.get("reason") != "compiler-message":
        continue
    m = m["message"]
    if m["level"] != "error" or (m.get("code") or {}).get("code") not in ("E0432", "E0603"):
        continue
    for sp in m["spans"]:
        name = sp["text"][0]["text"][sp["text"][0]["highlight_start"] - 1 : sp["text"][0]["highlight_end"] - 1]
        removals[sp["file_name"]].add((sp["byte_start"], sp["byte_end"], name))

total = 0
for f, items in removals.items():
    path = os.path.join(root, f)
    data = open(path, "rb").read()
    external = collections.defaultdict(lambda: collections.defaultdict(set))  # stmt_start -> crate -> names
    for start, end, name in sorted(items, reverse=True):
        seg = data[start:end].decode()
        if " as " in seg:
            print("alias, skip manual:", f, seg)
            continue
        leaf = seg.split("::")[-1]
        stmt = data.rfind(b"use ", 0, start)
        line_start = data.rfind(b"\n", 0, stmt) + 1
        if leaf not in defs or leaf in ROOT_EXPLICIT:
            external[line_start][ROOT_EXPLICIT.get(leaf, "worker")].add(leaf)
        j = end
        while j < len(data) and data[j : j + 1] in (b" ", b"\n"):
            j += 1
        if data[j : j + 1] == b",":
            j += 1
        data = data[:start] + data[j:]
        total += 1
    for pos in sorted(external, reverse=True):
        indent = re.match(rb"[ \t]*", data[pos:]).group(0).decode()
        lines = "".join(
            f"{indent}use {c}::{{{', '.join(sorted(n))}}};\n" for c, n in sorted(external[pos].items())
        )
        data = data[:pos] + lines.encode() + data[pos:]
    text = data.decode()
    text = re.sub(r"(?m)^\s*(pub\(crate\) )?use [a-z_:]+::\{\s*\};\n", "", text)
    text = re.sub(r"(?m)^\s*(pub\(crate\) )?use\s*;\n", "", text)
    text = re.sub(r"(?m)^\s*(pub\(crate\) )?use [a-z_:]+::;\n", "", text)
    open(path, "w").write(text)
print("removed", total, "names in", len(removals), "files")
