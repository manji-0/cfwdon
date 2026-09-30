#!/usr/bin/env python3
"""Iteratively apply rustc 'consider importing' suggestions until the crate resolves.

usage: fix_imports.py <repo_root> [--target wasm32-unknown-unknown] [--max-iter N]
"""
import collections
import json
import os
import re
import subprocess
import sys

root = sys.argv[1]
target = None
if "--target" in sys.argv:
    target = sys.argv[sys.argv.index("--target") + 1]
max_iter = int(sys.argv[sys.argv.index("--max-iter") + 1]) if "--max-iter" in sys.argv else 30

STD_PREFIX = ("std::", "core::", "alloc::")


def run_check():
    cmd = ["devbox", "run", "--", "cargo", "check", "-p", "cfwdon-worker", "--message-format=json"]
    if target:
        cmd += ["--target", target, "--lib"]
    else:
        cmd += ["--all-targets"]
    p = subprocess.run(cmd, cwd=root, capture_output=True, text=True)
    msgs = []
    for line in p.stdout.splitlines():
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get("reason") == "compiler-message":
            msgs.append(m["message"])
    return msgs


def rank(path):
    # lower is better
    s = path.strip()
    if s[4:].startswith(STD_PREFIX):
        return None
    if s.startswith("use crate::"):
        return (0, s.count("::"), len(s))
    for i, pre in enumerate(("use worker::", "use cfwdon_domain::", "use cfwdon_core::", "use serde")):
        if s.startswith(pre):
            return (1 + i, s.count("::"), len(s))
    return (9, s.count("::"), len(s))


def candidates(msg):
    out = []
    for ch in msg.get("children", []):
        for sp in ch.get("spans", []):
            rep = sp.get("suggested_replacement")
            if rep and rep.lstrip().startswith("use ") and rep.rstrip().endswith(";"):
                out.append((sp["file_name"], sp["byte_start"], rep))
    return out


def direct_refs(msg):
    out = []
    for ch in msg.get("children", []):
        if ch["message"].startswith("if you import"):
            for sp in ch.get("spans", []):
                if sp.get("suggested_replacement") is not None:
                    out.append((sp["file_name"], sp["byte_start"], sp["byte_end"], sp["suggested_replacement"]))
    return out


applied = set()
for it in range(max_iter):
    msgs = run_check()
    errors = [m for m in msgs if m["level"] == "error"]
    print(f"iter {it}: {len(errors)} errors", flush=True)
    if not errors:
        break
    inserts = collections.defaultdict(set)  # file -> {(pos, text)}
    edits = collections.defaultdict(set)  # file -> {(start, end, text)}
    for m in errors:
        cands = candidates(m)
        best = None
        for f, pos, rep in cands:
            r = rank(rep.strip())
            if r is None:
                continue
            if best is None or r < best[0]:
                best = (r, f, pos, rep)
        if best:
            _, f, pos, rep = best
            inserts[f].add((pos, rep))
            for ef, st, en, txt in direct_refs(m):
                edits[ef].add((st, en, txt))
            continue
        code = (m.get("code") or {}).get("code")
        sp0 = (m.get("spans") or [None])[0]
        if code == "E0107" and sp0 and sp0["text"] and "Result" in sp0["text"][0]["text"]:
            f = sp0["file_name"]
            txt = open(os.path.join(root, f)).read()
            mm = re.search(r"(?m)^use ", txt)
            pos = len(txt[: mm.start()].encode()) if mm else 0
            inserts[f].add((pos, "use worker::Result;\n"))
        elif code == "E0603":
            for ch in m.get("children", []):
                if ch["message"].startswith("import `"):
                    for sp in ch.get("spans", []):
                        if sp.get("suggested_replacement") is not None:
                            edits[sp["file_name"]].add((sp["byte_start"], sp["byte_end"], sp["suggested_replacement"]))
                            inserts.setdefault(sp["file_name"], set())
    if not inserts and not edits:
        codes = collections.Counter((m.get("code") or {}).get("code") for m in errors)
        print("no applicable suggestions; remaining codes:", codes.most_common(10))
        for m in errors[:15]:
            sp = (m.get("spans") or [{}])[0]
            print(" ", sp.get("file_name"), sp.get("line_start"), m["message"][:160])
        sys.exit(1)
    n = 0
    for f in set(inserts) | set(edits):
        items = inserts.get(f, ())
        path = os.path.join(root, f)
        data = open(path, "rb").read()
        # group by position, insert from the end so offsets stay valid
        ops = []  # (pos, order, kind, payload)
        for st, en, txt in edits.pop(f, ()):
            ops.append((st, 1, "edit", (en, txt)))
        seen = set()
        for pos, rep in items:
            # same text already present right at the insertion point -> skip
            if data[pos : pos + len(rep.encode())] != rep.encode() and rep not in seen and (f, rep, data[:pos].count(b"\n")) not in applied:
                applied.add((f, rep, data[:pos].count(b"\n")))
                seen.add(rep)
                ops.append((pos, 0, "ins", rep))
        last = None
        for pos, _, kind, payload in sorted(ops, key=lambda o: (o[0], o[1]), reverse=True):
            if kind == "edit":
                en, txt = payload
                if last is not None and en > last:
                    continue
                data = data[:pos] + txt.encode() + data[en:]
                last = pos
            else:
                n += 1
                data = data[:pos] + payload.encode() + data[pos:]
        open(path, "wb").write(data)
    print(f"  inserted {n} imports in {len(inserts)} files", flush=True)
else:
    print("max iterations reached")
    sys.exit(2)
