#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";
import {
  normalizeInstanceOrigin,
  parseDevArgs,
  webUiDevUsage,
} from "./lib/dev_instance.mjs";
import { printLocalAuth0Setup } from "./lib/dev_auth0.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const defaultOrigin = "http://127.0.0.1:8787";

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    stdio: "inherit",
    env: process.env,
    ...options,
  });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

const parsed = parseDevArgs(process.argv.slice(2));
if (parsed.help) {
  process.stdout.write(webUiDevUsage());
  process.exit(0);
}

const origin = parsed.instance ? normalizeInstanceOrigin(parsed.instance) : defaultOrigin;
process.stdout.write(`Proxying API routes to ${origin}\n`);
if (origin.startsWith("https://")) {
  process.stdout.write(
    "Note: Auth0 login callbacks stay on the remote host. Use local worker:dev for full login, or worker:dev --remote.\n",
  );
} else {
  printLocalAuth0Setup();
}

run(
  "pnpm",
  ["run", "dev"],
  {
    cwd: path.join(repoRoot, "web-ui"),
    env: {
      ...process.env,
      CFWDON_DEV_ORIGIN: origin,
    },
  },
);
