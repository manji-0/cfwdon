const WORKER_CRATE = "crates/cfwdon-worker";

function truthyDev(value) {
  const normalized = (value ?? "").trim().toLowerCase();
  return normalized === "1" || normalized === "true" || normalized === "yes" || normalized === "dev";
}

/**
 * Choose worker-build --dev vs --release.
 *
 * Default is --release so `wrangler deploy` and CI dry-run keep wasm-opt.
 * `WORKER_BUILD_PROFILE` wins when set. Otherwise `WRANGLER_DEV` selects --dev.
 */
export function workerBuildFlag(env = process.env) {
  const profile = (env.WORKER_BUILD_PROFILE ?? "").trim().toLowerCase();
  if (profile === "dev" || profile === "--dev") {
    return "--dev";
  }
  if (profile === "release" || profile === "--release") {
    return "--release";
  }
  if (profile) {
    throw new Error(
      `cfwdon: unknown WORKER_BUILD_PROFILE=${profile} (use dev or release)`,
    );
  }
  if (truthyDev(env.WRANGLER_DEV)) {
    return "--dev";
  }
  return "--release";
}

export function workerBuildArgs(env = process.env) {
  return [workerBuildFlag(env), WORKER_CRATE];
}
