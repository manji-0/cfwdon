#!/usr/bin/env node
import { cpSync, existsSync, mkdirSync, rmSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const assetsRoot = path.join(repoRoot, "assets");

const bundles = [
  {
    dist: path.join(repoRoot, "web-ui/dist"),
    dest: path.join(assetsRoot, "app"),
    fallback: path.join(repoRoot, "crates/cfwdon-worker/web_ui_fallback/index.html"),
  },
  {
    dist: path.join(repoRoot, "admin-ui/dist"),
    dest: path.join(assetsRoot, "admin"),
    fallback: path.join(repoRoot, "crates/cfwdon-worker/admin_ui_fallback/index.html"),
  },
];

function stageBundle({ dist, dest, fallback }) {
  rmSync(dest, { recursive: true, force: true });
  mkdirSync(dest, { recursive: true });
  if (existsSync(dist)) {
    cpSync(dist, dest, { recursive: true });
    return;
  }
  cpSync(fallback, path.join(dest, "index.html"));
}

export function stageUiAssets() {
  mkdirSync(assetsRoot, { recursive: true });
  for (const bundle of bundles) {
    stageBundle(bundle);
  }
}

const isDirectRun = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isDirectRun) {
  stageUiAssets();
}
