import test from "node:test";
import assert from "node:assert/strict";
import {
  instanceDomainFromOrigin,
  normalizeInstanceOrigin,
  parseDevArgs,
} from "./dev_instance.mjs";

test("normalizeInstanceOrigin adds https scheme", () => {
  assert.equal(normalizeInstanceOrigin("fedi.manji.app"), "https://fedi.manji.app");
  assert.equal(normalizeInstanceOrigin("https://mastodon.social/"), "https://mastodon.social");
  assert.equal(normalizeInstanceOrigin("http://127.0.0.1:8787"), "http://127.0.0.1:8787");
});

test("instanceDomainFromOrigin keeps non-default ports", () => {
  assert.equal(instanceDomainFromOrigin("http://127.0.0.1:8787"), "127.0.0.1:8787");
  assert.equal(instanceDomainFromOrigin("https://fedi.manji.app"), "fedi.manji.app");
});

test("parseDevArgs accepts positional instance", () => {
  assert.deepEqual(parseDevArgs(["--remote", "fedi.manji.app"]), {
    help: false,
    remote: true,
    instance: "fedi.manji.app",
    skipWebUiBuild: false,
  });
});
