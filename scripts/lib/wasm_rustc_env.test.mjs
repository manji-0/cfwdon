import test from "node:test";
import assert from "node:assert/strict";
import path from "node:path";
import { wasmRustcEnv, whichOnPath } from "./wasm_rustc_env.mjs";

const repoRoot = "/tmp/cfwdon-repo";
const toolchainBin = path.join(
  repoRoot,
  ".devbox",
  ".rustup",
  "toolchains",
  "stable-x86_64-unknown-linux-gnu",
  "bin",
);
const rustc = path.join(toolchainBin, "rustc");
const cargo = path.join(toolchainBin, "cargo");

function fakeRun(command, args) {
  const joined = args.join(" ");
  if (joined === "which --toolchain stable rustc") {
    return { status: 0, stdout: `${rustc}\n`, stderr: "", error: null };
  }
  if (joined === "which --toolchain stable cargo") {
    return { status: 0, stdout: `${cargo}\n`, stderr: "", error: null };
  }
  if (joined === "target list --installed --toolchain stable") {
    return {
      status: 0,
      stdout: "x86_64-unknown-linux-gnu\nwasm32-unknown-unknown\n",
      stderr: "",
      error: null,
    };
  }
  return { status: 1, stdout: "", stderr: `unexpected ${command} ${joined}`, error: null };
}

test("whichOnPath finds the first existing binary", () => {
  const found = whichOnPath("node", process.env.PATH);
  assert.ok(found);
  assert.ok(found.endsWith(`${path.sep}node`));
});

test("wasmRustcEnv prepends repo rustup rustc ahead of host cargo", () => {
  const hostCargo = "/usr/local/cargo/bin";
  const env = wasmRustcEnv({
    repoRoot,
    env: {
      PATH: `${hostCargo}:/usr/bin:/bin`,
      RUSTUP: process.execPath,
    },
    run: fakeRun,
  });

  const entries = env.PATH.split(path.delimiter);
  assert.equal(entries[0], toolchainBin);
  assert.ok(entries.includes(hostCargo));
  assert.ok(entries.includes("/usr/bin"));
  assert.equal(env.RUSTC, rustc);
  assert.equal(env.CARGO, cargo);
  assert.equal(env.RUSTUP_HOME, path.join(repoRoot, ".devbox", ".rustup"));
  assert.equal(env.CARGO_HOME, path.join(repoRoot, ".devbox", ".cargo"));
  assert.notEqual(env.RUSTUP_HOME, "/usr/local/rustup");
});

test("wasmRustcEnv keeps host git on PATH", () => {
  const env = wasmRustcEnv({
    repoRoot,
    env: {
      PATH: "/usr/bin:/bin",
      RUSTUP: process.execPath,
    },
    run: fakeRun,
  });
  assert.ok(env.PATH.split(path.delimiter).includes("/usr/bin"));
});

test("wasmRustcEnv rejects a toolchain without wasm32", () => {
  assert.throws(
    () =>
      wasmRustcEnv({
        repoRoot,
        env: { PATH: "/usr/bin", RUSTUP: process.execPath },
        run: (command, args) => {
          if (args.join(" ").startsWith("target list")) {
            return {
              status: 0,
              stdout: "x86_64-unknown-linux-gnu\n",
              stderr: "",
              error: null,
            };
          }
          return fakeRun(command, args);
        },
      }),
    /wasm32-unknown-unknown/,
  );
});

test("wasmRustcEnv tells macOS operators not to wrap git via devbox run", () => {
  assert.throws(
    () =>
      wasmRustcEnv({
        repoRoot,
        env: { PATH: "/usr/bin", RUSTUP: process.execPath },
        run: () => ({
          status: 1,
          stdout: "",
          stderr: "no rustc",
          error: null,
        }),
      }),
    /do not use `devbox run -- wrangler`/,
  );
});
