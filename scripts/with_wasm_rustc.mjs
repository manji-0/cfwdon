#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { wasmRustcEnv } from "./lib/wasm_rustc_env.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const command = process.argv[2];
const args = process.argv.slice(3);

if (!command || command === "--help" || command === "-h") {
  process.stdout.write(
    "Usage: node scripts/with_wasm_rustc.mjs <command> [args...]\n" +
      "Runs a command with the repo rustup toolchain (wasm32-unknown-unknown) first on PATH.\n" +
      "Example: node scripts/with_wasm_rustc.mjs wrangler deploy\n" +
      "On macOS do not use: devbox run -- wrangler\n",
  );
  process.exit(command ? 0 : 1);
}

let env;
try {
  env = wasmRustcEnv({ repoRoot });
} catch (error) {
  process.stderr.write(`${error.message}\n`);
  process.exit(1);
}

const result = spawnSync(command, args, {
  cwd: repoRoot,
  env,
  stdio: "inherit",
});
process.exit(result.status ?? 1);
