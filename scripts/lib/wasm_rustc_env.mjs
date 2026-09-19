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

function rustupFailMessage(detail) {
  return [
    `cfwdon: ${detail}`,
    "Install the repo rustup toolchain with `devbox shell` (wasm32-unknown-unknown).",
    "Then run `wrangler deploy` from that shell, or `eval \"$(devbox shellenv)\"` followed by wrangler.",
    "On macOS do not use `devbox run -- wrangler` — that wraps git and hits the Xcode license.",
  ].join("\n");
}

function requireStatus(result, label) {
  if (result.error) {
    throw new Error(rustupFailMessage(`${label} failed: ${result.error.message}`));
  }
  if (result.status !== 0) {
    const stderr = (result.stderr ?? "").trim();
    throw new Error(
      rustupFailMessage(`${label} failed${stderr ? `: ${stderr}` : ""}`),
    );
  }
  return (result.stdout ?? "").trim();
}

export function resolveRustupBin(repoRoot, env = process.env) {
  if (env.RUSTUP && existsSync(env.RUSTUP)) {
    return env.RUSTUP;
  }
  const fromPath = whichOnPath("rustup", env.PATH);
  if (fromPath) {
    return fromPath;
  }
  const fromDevbox = path.join(
    repoRoot,
    ".devbox",
    "nix",
    "profile",
    "default",
    "bin",
    "rustup",
  );
  if (existsSync(fromDevbox)) {
    return fromDevbox;
  }
  throw new Error(rustupFailMessage("rustup is not on PATH"));
}

/**
 * Build an env overlay so wrangler / worker-build use the repo rustup
 * toolchain that has wasm32-unknown-unknown, not a host or Nix rustc.
 */
export function wasmRustcEnv({
  repoRoot,
  env = process.env,
  run = defaultRun,
} = {}) {
  if (!repoRoot) {
    throw new Error("cfwdon: wasmRustcEnv requires repoRoot");
  }

  const rustupHome = path.join(repoRoot, ".devbox", ".rustup");
  const cargoHome = path.join(repoRoot, ".devbox", ".cargo");
  const toolchain = env.RUSTUP_TOOLCHAIN || "stable";
  const rustupEnv = {
    ...env,
    RUSTUP_HOME: rustupHome,
    CARGO_HOME: cargoHome,
    RUSTUP_TOOLCHAIN: toolchain,
  };

  const rustupBin = resolveRustupBin(repoRoot, rustupEnv);
  const rustc = requireStatus(
    run(rustupBin, ["which", "--toolchain", toolchain, "rustc"], rustupEnv),
    "rustup which rustc",
  );
  const cargo = requireStatus(
    run(rustupBin, ["which", "--toolchain", toolchain, "cargo"], rustupEnv),
    "rustup which cargo",
  );
  const installedTargets = requireStatus(
    run(
      rustupBin,
      ["target", "list", "--installed", "--toolchain", toolchain],
      rustupEnv,
    ),
    "rustup target list",
  );
  const hasWasm = installedTargets
    .split(/\r?\n/)
    .some((line) => line.trim() === WASM_TARGET);
  if (!hasWasm) {
    throw new Error(
      rustupFailMessage(
        `${rustc} has no ${WASM_TARGET} target (RUSTUP_HOME=${rustupHome})`,
      ),
    );
  }

  const rustcDir = path.dirname(rustc);
  const cargoDir = path.dirname(cargo);
  const cargoBin = path.join(cargoHome, "bin");
  const prepend = [rustcDir, cargoDir, cargoBin].filter((dir, index, dirs) => {
    return dirs.indexOf(dir) === index;
  });
  const nextPath = [...prepend, ...pathEntries(env.PATH)].join(path.delimiter);

  return {
    ...rustupEnv,
    PATH: nextPath,
    RUSTC: rustc,
    CARGO: cargo,
    RUSTUP: rustupBin,
  };
}
