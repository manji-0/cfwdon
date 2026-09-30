#!/usr/bin/env python3
"""Guard the cfwdon-worker module layering.

The worker crate is layered as

    foundation (infra helpers, DTOs, auth/session, store::*)
        <- feature modules (statuses, timelines, notifications, ...)
        <- routing / router / lib.rs entry points

This check keeps that shape from eroding:

- foundation modules may only import other foundation modules;
- the crate root must not re-export modules with globs, and no module may
  glob-import the crate root (both hide real dependencies);
- the largest cycle among top-level modules must not grow past its
  recorded size. Lower MAX_CYCLE_MODULES whenever a refactor shrinks it.
"""

from __future__ import annotations

import re
import sys
from collections import defaultdict
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SRC = REPO_ROOT / "crates/cfwdon-worker/src"

FOUNDATION = {
    "app_cache",
    "auth",
    "content_helpers",
    "crypto_keys",
    "d1_metrics",
    "db_session",
    "db_utils",
    "federation",
    "id_utils",
    "identity",
    "oauth_store",
    "observability",
    "policy_documents",
    "public_endpoint_cache",
    "request_utils",
    "response_cache",
    "response_utils",
    "responses",
    "runtime_config",
    "secret_storage",
    "store",
    "stream_hub",
    "streaming_types",
    "time_html",
    "tracked_d1",
    "trends_cache",
    "ui_assets",
}
MAX_CYCLE_MODULES = 33

TEST_MODULE_RE = re.compile(r"#\[cfg\(test\)\]")
CRATE_PATH_RE = re.compile(r"\bcrate::([a-z_][a-z0-9_]*)")
ROOT_GLOB_RE = re.compile(r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?use\s+[a-z_][a-z0-9_]*::\*;")
CRATE_GLOB_RE = re.compile(r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?use\s+crate::\*;")


def top_module(path: Path) -> str:
    rel = path.relative_to(SRC)
    return rel.parts[0].removesuffix(".rs")


def is_test_file(path: Path) -> bool:
    return path.stem == "unit_tests" or path.stem.endswith("_tests")


def module_graph() -> tuple[dict[str, set[str]], set[str]]:
    edges: dict[str, set[str]] = defaultdict(set)
    modules: set[str] = set()
    for path in SRC.rglob("*.rs"):
        module = top_module(path)
        if module in {"lib", "compat_tests", "test_fixtures"} or is_test_file(path):
            continue
        modules.add(module)
        # Test modules sit at the end of files; they may reach anywhere.
        production = TEST_MODULE_RE.split(path.read_text(), maxsplit=1)[0]
        for target in CRATE_PATH_RE.findall(production):
            if target != module:
                edges[module].add(target)
    return edges, modules


def largest_cycle(edges: dict[str, set[str]], modules: set[str]) -> set[str]:
    index: dict[str, int] = {}
    low: dict[str, int] = {}
    stack: list[str] = []
    on_stack: set[str] = set()
    best: set[str] = set()
    counter = 0
    sys.setrecursionlimit(10_000)

    def visit(node: str) -> None:
        nonlocal counter, best
        index[node] = low[node] = counter
        counter += 1
        stack.append(node)
        on_stack.add(node)
        for succ in edges[node]:
            if succ not in modules:
                continue
            if succ not in index:
                visit(succ)
                low[node] = min(low[node], low[succ])
            elif succ in on_stack:
                low[node] = min(low[node], index[succ])
        if low[node] == index[node]:
            component = set()
            while True:
                member = stack.pop()
                on_stack.discard(member)
                component.add(member)
                if member == node:
                    break
            if len(component) > len(best):
                best = component

    for module in sorted(modules):
        if module not in index:
            visit(module)
    return best


def main() -> int:
    errors: list[str] = []
    edges, modules = module_graph()

    missing = FOUNDATION - modules
    if missing:
        errors.append(f"foundation modules not found: {sorted(missing)}")

    for module in sorted(FOUNDATION & modules):
        upward = sorted(target for target in edges[module] if target in modules - FOUNDATION)
        if upward:
            errors.append(f"foundation module `{module}` imports feature modules: {upward}")

    lib_rs = (SRC / "lib.rs").read_text()
    if ROOT_GLOB_RE.search(lib_rs):
        errors.append("lib.rs must not glob re-export modules; import items explicitly")
    for path in sorted(SRC.rglob("*.rs")):
        if CRATE_GLOB_RE.search(path.read_text()):
            errors.append(f"{path.relative_to(REPO_ROOT)} glob-imports the crate root")

    cycle = largest_cycle(edges, modules)
    if len(cycle) > MAX_CYCLE_MODULES:
        errors.append(
            f"largest module cycle grew to {len(cycle)} modules (limit {MAX_CYCLE_MODULES}): "
            f"{sorted(cycle)}"
        )

    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1
    print(
        f"module layer check ok: {len(FOUNDATION)} foundation modules, "
        f"largest cycle {len(cycle)}/{MAX_CYCLE_MODULES} of {len(modules)} modules"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
