#!/usr/bin/env python3
"""Split a `wrangler d1 export` dump into parts that import into a fresh D1.

A whole dump fails to import (`D1_RESET_DO`), and the dump inserts rows before
later tables exist, which D1's enforced foreign keys reject. This writes
part00 (CREATE TABLE), partNN (rows, parent tables first, about 1 MB each),
and partZZ (sqlite_sequence, indexes, triggers) into OUTDIR.

Usage: python3 scripts/split_d1_export.py export.sql OUTDIR
"""
import re, sqlite3, sys, collections, os
src, outdir = sys.argv[1], sys.argv[2]
os.makedirs(outdir, exist_ok=True)
stmts, buf = [], ""
for line in open(src, encoding="utf-8"):
    buf += line
    if sqlite3.complete_statement(buf):
        stmts.append(buf.strip()); buf = ""
tables, data, tail, pragma = [], collections.defaultdict(list), [], []
for s in stmts:
    u = s.upper()
    if u.startswith("PRAGMA"): pragma.append(s)
    elif u.startswith("CREATE TABLE"): tables.append(s)
    elif u.startswith("INSERT") and "SQLITE_SEQUENCE" not in u[:60]:
        m = re.match(r'INSERT INTO\s+"?([A-Za-z0-9_]+)"?', s)
        data[m.group(1)].append(s)
    else: tail.append(s)
# FK graph from a scratch db built from the CREATE TABLE statements.
db = sqlite3.connect(":memory:")
for s in tables: db.execute(s)
names = [r[0] for r in db.execute("select name from sqlite_master where type='table'")]
deps = {n: {r[2] for r in db.execute(f'pragma foreign_key_list("{n}")') if r[2] != n} for n in names}
order, seen = [], set()
def visit(n, stack=()):
    if n in seen: return
    if n in stack: return
    for d in deps.get(n, ()): visit(d, stack + (n,))
    seen.add(n); order.append(n)
for n in names: visit(n)
def write(name, items):
    with open(os.path.join(outdir, name), "w", encoding="utf-8") as f:
        for p in pragma: f.write(p + "\n")
        for s in items: f.write(s + "\n")
write("part00_tables.sql", tables)
chunk, size, idx = [], 0, 1
for t in order + [t for t in data if t not in order]:
    for s in data.get(t, []):
        chunk.append(s); size += len(s)
        if size > 1_000_000:
            write(f"part{idx:02d}_data.sql", chunk); idx += 1; chunk, size = [], 0
if chunk: write(f"part{idx:02d}_data.sql", chunk)
write("partZZ_tail.sql", tail)
print("tables", len(tables), "data parts", idx, "tail", len(tail), "rows", sum(len(v) for v in data.values()))
