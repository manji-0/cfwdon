# Worker refactor tools
<!-- constrained-by ../../docs/getting-started/development.md#worker-module-layers -->

Compiler-driven helpers used to remove the crate-root glob namespace and to move repositories into the `store` layer of `crates/cfwdon-worker`. They edit sources in place; run them from the repository root on a clean tree so every step can be reviewed with `git diff`.

| Script | Purpose |
| --- | --- |
| `promote.py <src> <feature> <sub>...` | Move `src/<feature>/<sub>.rs` into `store/<feature>/` and rewrite the parent/sibling paths. |
| `move_items.py <src.rs> <dst.rs> [--pub] NAME...` | Move top-level items (with docs/attributes) between files; `--pub` widens them to `pub(crate)`. |
| `pull_deps.py <root> <dst.rs> <src.rs>...` | Repeatedly pull the private helpers a moved file still needs from the listed sources. |
| `strip_unresolved.py <root> [target]` | Remove names rustc reports as unresolved (E0432/E0603) from `use` lists; re-import external names from their crate. |
| `fix_imports.py <root> [--target T]` | Apply rustc's "consider importing" suggestions until the crate resolves. |
| `fix_docs.py` | Move prepended `use` lines below inner `//!` docs and `#![...]` attributes. |
| `merge_uses.py <worker_src>` | Merge `use a::X; use a::Y;` runs into `use a::{X, Y};` before `cargo fmt`. |
| `settle.sh` | Run the resolve/fix loop, `cargo fix`, formatting, both clippies, tests, and `check_module_layers.py`. |

Typical move of a repository used by several features:

```sh
python3 scripts/refactor/promote.py crates/cfwdon-worker/src remote actor_store
scripts/refactor/settle.sh
```

Review the diff, especially match arms on constants and same-name functions: clippy's `unreachable_patterns` and `unconditional_recursion` warnings flag imports that resolved to something other than before.
