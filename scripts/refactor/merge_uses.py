#!/usr/bin/env python3
"""Merge runs of `use prefix::Item;` statements that share a prefix (rustfmt 'Module' granularity)."""
import glob
import re
import sys

root = sys.argv[1]
USE_RE = re.compile(r"^(\s*)((?:pub(?:\(crate\))? )?)use ((?:[A-Za-z_][A-Za-z0-9_]*::)*)(.+);$")


def collapse(text):
    # join multi-line `use ...{ ... };` into one line when braces are not nested
    out = []
    lines = text.split("\n")
    i = 0
    while i < len(lines):
        line = lines[i]
        if re.match(r"^\s*(pub(\(crate\))? )?use .*\{\s*$", line):
            j = i
            buf = [line]
            while j + 1 < len(lines) and not lines[j].rstrip().endswith("};"):
                j += 1
                buf.append(lines[j])
            joined = buf[0].rstrip() + " ".join(b.strip() for b in buf[1:])
            if joined.count("{") == 1 and joined.rstrip().endswith("};") and "//" not in joined:
                joined = re.sub(r",\s*\}", "}", joined)
                out.append(joined)
                i = j + 1
                continue
        out.append(line)
        i += 1
    return "\n".join(out)


def items_of(tail):
    tail = tail.strip()
    if tail.startswith("{"):
        if tail.count("{") != 1:
            return None
        return [t.strip() for t in tail[1:-1].split(",") if t.strip()]
    return [tail]


def merge(text):
    lines = text.split("\n")
    out = []
    i = 0
    while i < len(lines):
        m = USE_RE.match(lines[i])
        prev = out[-1].strip() if out else ""
        if not m or prev.startswith("#["):
            out.append(lines[i])
            i += 1
            continue
        run = []
        while i < len(lines):
            m = USE_RE.match(lines[i])
            if not m:
                break
            if i + 1 < len(lines) and False:
                pass
            run.append(m)
            i += 1
            # stop the run before an attribute-guarded use
            if i < len(lines) and lines[i].strip().startswith("#["):
                break
        groups = {}
        order = []
        passthrough = []
        for m in run:
            indent, vis, prefix, tail = m.groups()
            its = items_of(tail)
            if not prefix or its is None or any("{" in t for t in its):
                passthrough.append(m.group(0))
                continue
            key = (indent, vis, prefix)
            if key not in groups:
                groups[key] = []
                order.append(key)
            for t in its:
                if t not in groups[key]:
                    groups[key].append(t)
        for key in order:
            indent, vis, prefix = key
            its = groups[key]
            body = its[0] if len(its) == 1 else "{" + ", ".join(its) + "}"
            out.append(f"{indent}{vis}use {prefix}{body};")
        out.extend(passthrough)
    return "\n".join(out)


changed = 0
for p in glob.glob(f"{root}/**/*.rs", recursive=True):
    s = open(p).read()
    n = merge(collapse(s))
    if n != s:
        open(p, "w").write(n)
        changed += 1
print("files changed:", changed)
