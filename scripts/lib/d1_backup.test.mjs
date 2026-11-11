import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";
import { backupKey, parseBackupKey, selectPrunable } from "./d1_backup.mjs";

const script = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "d1_backup.mjs");
const options = { prefix: "d1/", database: "cfwdon" };
const key = (iso) => backupKey("d1/", "cfwdon", new Date(iso));
const DAY_MS = 24 * 60 * 60 * 1000;
const daily = (from, days) =>
  Array.from({ length: days }, (_, day) => key(new Date(Date.parse(from) + day * DAY_MS).toISOString()));

describe("backup keys", () => {
  it("names a backup by database and UTC second", () => {
    assert.equal(key("2026-11-09T03:43:07.512Z"), "d1/cfwdon-20261109T034307Z.sql.gz.age");
    assert.equal(
      backupKey("", "cfwdon-staging", new Date("2027-01-02T00:00:00Z")),
      "cfwdon-staging-20270102T000000Z.sql.gz.age",
    );
  });

  it("reads the time back from a key", () => {
    assert.equal(
      parseBackupKey("d1/", "cfwdon", key("2026-11-09T03:43:07Z"))?.toISOString(),
      "2026-11-09T03:43:07.000Z",
    );
  });

  it("does not take other objects for backups", () => {
    for (const other of [
      "d1/cfwdon-20261109T034307Z.sql",
      "d1/cfwdon-20261109T034307Z.sql.gz.age.tmp",
      "d1/cfwdon-staging-20261109T034307Z.sql.gz.age",
      "media/cfwdon-20261109T034307Z.sql.gz.age",
      "d1/cfwdon-20261340T034307Z.sql.gz.age",
      "d1/cfwdon-20260230T000000Z.sql.gz.age",
      "d1/cfwdon-2026119T034307Z.sql.gz.age",
    ]) {
      assert.equal(parseBackupKey("d1/", "cfwdon", other), undefined, other);
    }
  });
});

describe("retention", () => {
  it("keeps recent days and the first backup of each recent month", () => {
    const keys = daily("2025-09-01T03:43:00Z", 435);
    const now = new Date("2026-11-09T04:00:00Z");
    const prunable = new Set(selectPrunable(keys, { ...options, now }));
    const kept = keys.filter((candidate) => !prunable.has(candidate));
    const monthly = kept.filter((candidate) => /-\d{6}01T/.test(candidate));
    assert.deepEqual(
      monthly.map((candidate) => candidate.slice(10, 16)),
      ["202512", "202601", "202602", "202603", "202604", "202605", "202606", "202607", "202608", "202609", "202610", "202611"],
    );
    const recent = kept.filter((candidate) => candidate >= "d1/cfwdon-20261005");
    assert.ok(recent.length >= 35);
  });

  it("never prunes the newest backups, however old", () => {
    const keys = daily("2020-01-01T00:00:00Z", 10);
    const prunable = selectPrunable(keys, {
      ...options,
      now: new Date("2026-11-09T00:00:00Z"),
      keepLatest: 7,
      keepMonths: 0,
    });
    assert.deepEqual(prunable, keys.slice(0, 3));
  });

  it("never returns keys that are not backups", () => {
    const keys = [key("2020-01-01T00:00:00Z"), "d1/notes.txt", "other/cfwdon-20200101T000000Z.sql.gz.age"];
    const prunable = selectPrunable(keys, {
      ...options,
      now: new Date("2026-11-09T00:00:00Z"),
      keepLatest: 1,
      keepMonths: 0,
      keepDays: 0,
    });
    assert.deepEqual(prunable, []);
  });
});

describe("command line", () => {
  const run = (args, input = "") =>
    execFileSync(process.execPath, [script, ...args], { input, stdio: "pipe" }).toString();

  it("prints a key and the prunable keys", () => {
    assert.equal(
      run(["key", "--database", "cfwdon", "--at", "2026-11-09T03:43:07Z"]),
      "d1/cfwdon-20261109T034307Z.sql.gz.age\n",
    );
    const keys = ["2026-01-15", "2026-01-20", "2026-01-25"].map((day) => key(`${day}T00:00:00Z`));
    assert.equal(
      run(
        ["prune", "--database", "cfwdon", "--now", "2026-11-09T00:00:00Z", "--keep-days", "0", "--keep-months", "12", "--keep-latest", "1"],
        keys.join("\n"),
      ),
      `${keys[1]}\n`,
    );
  });

  it("rejects an unsafe database name", () => {
    assert.throws(() => run(["key", "--database", "a/b"]));
  });
});
