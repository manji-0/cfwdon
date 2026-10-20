#!/bin/sh
# Re-resolve imports after moving worker items/modules, then run the server lint + test gate.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
cd "$ROOT"
for _ in 1 2 3; do
  python3 "$HERE/strip_unresolved.py" . | tail -1
  python3 "$HERE/fix_docs.py" >/dev/null
  python3 "$HERE/fix_imports.py" . --max-iter 6 2>&1 | tail -3
done
python3 "$HERE/fix_imports.py" . --target wasm32-unknown-unknown --max-iter 4 2>&1 | tail -1
cargo fix -p cfwdon-worker --lib --tests --allow-dirty >/dev/null 2>&1
python3 "$HERE/merge_uses.py" crates/cfwdon-worker/src >/dev/null
cargo fmt --all
echo "--- clippy"; just clippy 2>&1 | grep -E "^(error|warning)" -A6 | head -30
echo "--- test"; just test 2>&1 | grep -E "FAILED|panicked|test result: ok. [0-9]+ passed"
python3 scripts/check_module_layers.py
