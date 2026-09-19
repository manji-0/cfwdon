import test from "node:test";
import assert from "node:assert/strict";
import {
  workerBuildArgs,
  workerBuildFlag,
} from "./worker_build_profile.mjs";

test("wrangler deploy / CI default to --release", () => {
  assert.equal(workerBuildFlag({}), "--release");
  assert.equal(workerBuildFlag({ WRANGLER_DEV: "", WORKER_BUILD_PROFILE: "" }), "--release");
  assert.equal(workerBuildFlag({ WRANGLER_DEV: "0" }), "--release");
  assert.equal(workerBuildFlag({ WRANGLER_DEV: "false" }), "--release");
  assert.equal(workerBuildFlag({ CI: "true" }), "--release");
  assert.deepEqual(workerBuildArgs({}), ["--release", "crates/cfwdon-worker"]);
});

test("WRANGLER_DEV selects --dev", () => {
  assert.equal(workerBuildFlag({ WRANGLER_DEV: "1" }), "--dev");
  assert.equal(workerBuildFlag({ WRANGLER_DEV: "true" }), "--dev");
  assert.equal(workerBuildFlag({ WRANGLER_DEV: "yes" }), "--dev");
});

test("WORKER_BUILD_PROFILE wins over WRANGLER_DEV", () => {
  assert.equal(
    workerBuildFlag({ WRANGLER_DEV: "1", WORKER_BUILD_PROFILE: "release" }),
    "--release",
  );
  assert.equal(
    workerBuildFlag({ WRANGLER_DEV: "0", WORKER_BUILD_PROFILE: "dev" }),
    "--dev",
  );
});

test("unknown WORKER_BUILD_PROFILE throws", () => {
  assert.throws(
    () => workerBuildFlag({ WORKER_BUILD_PROFILE: "fast" }),
    /unknown WORKER_BUILD_PROFILE/,
  );
});
