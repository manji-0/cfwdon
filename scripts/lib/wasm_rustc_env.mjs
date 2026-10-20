import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";

const WASM_TARGET = "wasm32-unknown-unknown";

function pathEntries(pathValue) {
  return (pathValue ?? "")
    .split(path.delimiter)
    .map((entry) => entry.trim())
    .filter(Boolean);
}

export function whichOnPath(command, pathValue) {
  for (const dir of pathEntries(pathValue)) {
    const candidate = path.join(dir, command);
    if (existsSync(candidate)) {
      return candidate;
    }
  }
  return null;
}

function defaultRun(command, args, env) {
  return spawnSync(command, args, {
    env,
    encoding: "utf8",
  });
}

function toolchainFailMessage(detail) {
  return [
    `cfwdon: ${detail}`,
    "Enter the Nix dev shell first: `nix develop` (or direnv with `use flake`).",
    "Then run `wrangler deploy` from that shell, or `nix develop -c wrangler deploy`.",
  ].join("\n");
}

function resolveTool(name, override, env) {
  if (override && existsSync(override)) {
    return override;
  }
  const fromPath = whichOnPath(name, env.PATH);
  if (fromPath) {
    return fromPath;
  }
  throw new Error(toolchainFailMessage(`${name} is not on PATH`));
}

/**
 * Build an env overlay so wrangler / worker-build use a rustc that has
 * wasm32-unknown-unknown (the Nix dev shell toolchain), and fail early with
 * a clear message when the rustc on PATH lacks that target.
 */
export function wasmRustcEnv({
  repoRoot,
  env = process.env,
  run = defaultRun,
  exists = existsSync,
} = {}) {
  if (!repoRoot) {
    throw new Error("cfwdon: wasmRustcEnv requires repoRoot");
  }

  const rustc = resolveTool("rustc", env.RUSTC, env);
  const cargo = resolveTool("cargo", env.CARGO, env);

  const result = run(rustc, ["--print", "target-libdir", "--target", WASM_TARGET], env);
  if (result.error) {
    throw new Error(toolchainFailMessage(`${rustc} failed: ${result.error.message}`));
  }
  const libdir = (result.stdout ?? "").trim();
  if (result.status !== 0 || !libdir || !exists(libdir)) {
    throw new Error(toolchainFailMessage(`${rustc} has no ${WASM_TARGET} target`));
  }

  return {
    ...env,
    RUSTC: rustc,
    CARGO: cargo,
  };
}
