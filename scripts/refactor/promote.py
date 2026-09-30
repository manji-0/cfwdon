#!/usr/bin/env python3
"""Move feature submodules into the crate-level `store/<feature>/` layer.

usage: promote.py <worker_src_dir> <feature> <submodule>...
  e.g. promote.py crates/cfwdon-worker/src remote actor_profile_store poll_store
"""
import os
import re
import subprocess
import sys

src, feature, subs = sys.argv[1], sys.argv[2], sys.argv[3:]


def parent_file(feature):
    for cand in (f"{feature}.rs", f"{feature}/mod.rs"):
        if os.path.exists(os.path.join(src, cand)):
            return cand
    raise SystemExit(f"no parent for {feature}")


def ensure_mod(path, name, facade=True):
    full = os.path.join(src, path)
    text = open(full).read() if os.path.exists(full) else ""
    if not re.search(rf"(?m)^(pub\(crate\) )?mod {name};", text):
        lines = text.split("\n") if text else []
        mods = [i for i, l in enumerate(lines) if re.match(r"^(pub\(crate\) )?mod \w+;", l)]
        at = (mods[-1] + 1) if mods else 0
        lines.insert(at, f"mod {name};")
        if facade:
            uses = [i for i, l in enumerate(lines) if l.startswith("pub(crate) use ")]
            at2 = (uses[-1] + 1) if uses else at + 1
            lines.insert(at2, f"pub(crate) use {name}::*;")
        text = "\n".join(lines)
        if not text.endswith("\n"):
            text += "\n"
        open(full, "w").write(text)


pf = parent_file(feature)
ptext = open(os.path.join(src, pf)).read()
os.makedirs(os.path.join(src, "store", feature), exist_ok=True)
for sub in subs:
    subprocess.run(["git", "mv", f"{sub}.rs", f"../store/{feature}/{sub}.rs"],
                   cwd=os.path.join(src, feature), check=True)
    if os.path.isdir(os.path.join(src, feature, sub)):
        subprocess.run(["git", "mv", sub, f"../store/{feature}/{sub}"],
                       cwd=os.path.join(src, feature), check=True)
    ptext = re.sub(rf"(?m)^(pub(\([^)]*\))? )?mod {sub};\n", "", ptext)
    # facade / private imports of the moved module now come from the store layer
    ptext = re.sub(rf"(?m)^pub(\([^)]*\))? use {sub}::\*;\n", "", ptext)
    ptext = re.sub(rf"(?m)^(pub(\([^)]*\))? )?use {sub}::", f"use crate::store::{feature}::", ptext)
    ensure_mod(f"store/{feature}/mod.rs", sub)
open(os.path.join(src, pf), "w").write(ptext)
# sibling files reach moved modules through `super::<sub>::`
for dirpath, _, files in os.walk(os.path.join(src, feature)):
    for fn in files:
        fp = os.path.join(dirpath, fn)
        t = open(fp).read()
        n = t
        for sub in subs:
            n = re.sub(rf"\bsuper::{sub}::", f"crate::store::{feature}::", n)
        if n != t:
            open(fp, "w").write(n)
ensure_mod("store/mod.rs", feature, facade=False)
lib = open(os.path.join(src, "lib.rs")).read()
if not re.search(r"(?m)^mod store;", lib):
    lib = re.sub(r"(?m)^mod stream_hub;", "mod store;\nmod stream_hub;", lib, count=1)
    open(os.path.join(src, "lib.rs"), "w").write(lib)
# store/mod.rs exposes feature stores to the crate
sm = os.path.join(src, "store/mod.rs")
t = open(sm).read()
t = re.sub(r"(?m)^mod (\w+);", r"pub(crate) mod \1;", t)
open(sm, "w").write(t)
print("promoted", subs, "from", feature)
