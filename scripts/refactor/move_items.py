#!/usr/bin/env python3
"""Move top-level Rust items (with their doc comments / attributes) between files.

usage: move_items.py <src.rs> <dst.rs> [--pub] NAME [NAME...]
  --pub  raise private / pub(in ...) / pub(super) visibility of moved items to pub(crate)
"""
import re
import sys

args = sys.argv[1:]
src, dst = args[0], args[1]
raise_vis = "--pub" in args
names = [a for a in args[2:] if a != "--pub"]

ITEM_RE = re.compile(
    r"^(?P<vis>pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?"
    r"(?P<kind>fn|struct|enum|type|trait|const|static|impl)\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)"
)


def item_span(lines, i):
    """Return (start, end) line indexes (end exclusive) for the item starting at line i."""
    start = i
    while start > 0 and (lines[start - 1].startswith("///") or lines[start - 1].startswith("#[")):
        start -= 1
    depth = 0
    seen_open = False
    j = i
    while j < len(lines):
        line = re.sub(r'"(?:\\.|[^"\\])*"', '""', lines[j])
        line = re.sub(r"'(?:\\.|[^'\\])'", "''", line)
        line = line.split("//")[0]
        depth += line.count("{") - line.count("}")
        if "{" in line:
            seen_open = True
        if (seen_open and depth == 0) or (not seen_open and line.rstrip().endswith(";") and depth == 0):
            return start, j + 1
        j += 1
    raise SystemExit(f"unterminated item at line {i + 1}")


text = open(src).read()
lines = text.split("\n")
moved = []
remaining = set(names)
i = 0
out = []
while i < len(lines):
    m = ITEM_RE.match(lines[i])
    if m and m.group("name") in names and m.group("kind") != "impl":
        start, end = item_span(lines, i)
        # drop already-emitted doc/attr lines belonging to this item
        k = len(out) - (i - start)
        block = lines[start:end]
        del out[k:]
        if raise_vis:
            idx = i - start
            block[idx] = re.sub(r"^pub\((?:in [^)]*|super|self)\)\s+", "", block[idx])
            if not block[idx].startswith("pub"):
                block[idx] = "pub(crate) " + block[idx]
        moved.append("\n".join(block))
        remaining.discard(m.group("name"))
        i = end
        while i < len(lines) and lines[i] == "" and out and out[-1] == "":
            i += 1
        continue
    out.append(lines[i])
    i += 1

if remaining:
    print("not found:", sorted(remaining))
open(src, "w").write("\n".join(out))
d = open(dst).read() if __import__("os").path.exists(dst) else ""
d = d.rstrip("\n") + ("\n\n" if d.strip() else "") + "\n\n".join(moved) + "\n"
open(dst, "w").write(d)
print(f"moved {len(moved)} items -> {dst}")
