#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { stageUiAssets } from "./stage_ui_assets.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

stageUiAssets();

const result = spawnSync("worker-build", ["--release", "crates/cfwdon-worker"], {
  cwd: repoRoot,
  stdio: "inherit",
  env: process.env,
});
process.exit(result.status ?? 1);
