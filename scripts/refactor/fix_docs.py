#!/usr/bin/env python3
"""Move `use` lines that strip_unresolved.py prepended above inner doc comments / attributes.

Inner `//!` docs and `#![...]` attributes must come first in a file; run from the repo root.
"""
import glob
import re

for path in glob.glob("crates/cfwdon-worker/src/**/*.rs", recursive=True):
    lines = open(path).read().split("\n")
    i = 0
    while i < len(lines) and re.match(r"use (worker|serde|cfwdon_core|cfwdon_domain)::\{", lines[i]):
        i += 1
    if i == 0 or i >= len(lines) or not re.match(r"(//!|#!\[)", lines[i]):
        continue
    j = i
    while j < len(lines) and (lines[j].startswith(("//!", "#![")) or lines[j] == ""):
        j += 1
    open(path, "w").write("\n".join(lines[i:j] + lines[:i] + lines[j:]))
    print("fixed", path)
