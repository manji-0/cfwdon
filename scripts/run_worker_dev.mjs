#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";
import {
  instanceDomainFromOrigin,
  normalizeInstanceOrigin,
  parseDevArgs,
  workerDevUsage,
} from "./lib/dev_instance.mjs";
import { printLocalAuth0Setup } from "./lib/dev_auth0.mjs";
import { wasmRustcEnv } from "./lib/wasm_rustc_env.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
let cachedWorkerEnv;

function workerEnv() {
  if (!cachedWorkerEnv) {
    try {
      cachedWorkerEnv = wasmRustcEnv({ repoRoot });
    } catch (error) {
      process.stderr.write(`${error.message}\n`);
      process.exit(1);
    }
  }
  return cachedWorkerEnv;
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    stdio: "inherit",
    ...options,
    env: {
      ...workerEnv(),
      ...options.env,
    },
  });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

function buildWebUi() {
  run("pnpm", ["run", "build"], { cwd: path.join(repoRoot, "web-ui") });
}

function applyLocalMigrations() {
  run("wrangler", ["d1", "migrations", "apply", "DB", "--local"]);
}

const parsed = parseDevArgs(process.argv.slice(2));
if (parsed.help) {
  process.stdout.write(workerDevUsage());
  process.exit(0);
}

if (!parsed.skipWebUiBuild) {
  buildWebUi();
}

const wranglerArgs = ["dev"];
if (parsed.remote) {
  wranglerArgs.push("--remote");
}

if (parsed.instance) {
  const origin = normalizeInstanceOrigin(parsed.instance);
  const domain = instanceDomainFromOrigin(origin);
  wranglerArgs.push("--var", `INSTANCE_DOMAIN:${domain}`);
  wranglerArgs.push("--var", `AUTH0_AUDIENCE:https://${domain}/api`);
  wranglerArgs.push("--var", `AUTH0_EMAIL_CLAIM:https://${domain}/claims/email`);
  process.stdout.write(`Using instance domain: ${domain}\n`);
  if (origin.startsWith("http://127.0.0.1") || origin.startsWith("http://localhost")) {
    process.stdout.write(`Upstream origin: ${origin}\n`);
  }
} else if (!parsed.remote) {
  applyLocalMigrations();
  printLocalAuth0Setup();
}

run("wrangler", wranglerArgs, {
  env: {
    WRANGLER_DEV: process.env.WRANGLER_DEV || "1",
    WORKER_BUILD_PROFILE: process.env.WORKER_BUILD_PROFILE || "dev",
  },
});
