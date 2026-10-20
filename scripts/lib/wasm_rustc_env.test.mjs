import test from "node:test";
import assert from "node:assert/strict";
import path from "node:path";
import { wasmRustcEnv, whichOnPath } from "./wasm_rustc_env.mjs";

const repoRoot = "/tmp/cfwdon-repo";
const wasmLibdir = "/nix/store/rust/lib/rustlib/wasm32-unknown-unknown/lib";

function fakeRun(command, args) {
  if (args.join(" ") === "--print target-libdir --target wasm32-unknown-unknown") {
    return { status: 0, stdout: `${wasmLibdir}\n`, stderr: "", error: null };
  }
  return { status: 1, stdout: "", stderr: `unexpected ${command}`, error: null };
}

function baseEnv() {
  return {
    PATH: "/usr/bin:/bin",
    RUSTC: process.execPath,
    CARGO: process.execPath,
  };
}

test("whichOnPath finds the first existing binary", () => {
  const found = whichOnPath("node", process.env.PATH);
  assert.ok(found);
  assert.ok(found.endsWith(`${path.sep}node`));
});

test("wasmRustcEnv pins RUSTC and CARGO when rustc has wasm32", () => {
  const env = wasmRustcEnv({
    repoRoot,
    env: baseEnv(),
    run: fakeRun,
    exists: (dir) => dir === wasmLibdir,
  });
  assert.equal(env.RUSTC, process.execPath);
  assert.equal(env.CARGO, process.execPath);
  assert.equal(env.PATH, "/usr/bin:/bin");
});

test("wasmRustcEnv rejects a rustc without the wasm32 target", () => {
  assert.throws(
    () =>
      wasmRustcEnv({
        repoRoot,
        env: baseEnv(),
        run: fakeRun,
        exists: () => false,
      }),
    /wasm32-unknown-unknown/,
  );
});

test("wasmRustcEnv points operators at the Nix dev shell", () => {
  assert.throws(
    () =>
      wasmRustcEnv({
        repoRoot,
        env: baseEnv(),
        run: () => ({ status: 1, stdout: "", stderr: "no rustc", error: null }),
      }),
    /nix develop/,
  );
});
