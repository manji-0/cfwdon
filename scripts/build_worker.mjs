#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { stageUiAssets } from "./stage_ui_assets.mjs";
import { wasmRustcEnv } from "./lib/wasm_rustc_env.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

stageUiAssets();

let env;
try {
  env = wasmRustcEnv({ repoRoot });
} catch (error) {
  process.stderr.write(`${error.message}\n`);
  process.exit(1);
}

const result = spawnSync("worker-build", ["--release", "crates/cfwdon-worker"], {
  cwd: repoRoot,
  stdio: "inherit",
  env,
});
process.exit(result.status ?? 1);
