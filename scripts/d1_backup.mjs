#!/usr/bin/env node
// Object naming and retention for the scheduled D1 backups
// (.github/workflows/d1-backup.yml, docs/operations/runbook.md#scheduled-d1-backups).
//
//   node scripts/d1_backup.mjs key --prefix d1/ --database cfwdon-probe-5 [--at <ISO time>]
//     prints the object key for a backup taken now (or at --at).
//   node scripts/d1_backup.mjs prune --prefix d1/ --database cfwdon-probe-5 \
//       [--keep-days 35] [--keep-months 12] [--keep-latest 7] [--now <ISO time>] < keys
//     reads object keys (one per line) and prints the ones the retention
//     policy lets go, one per line. Keys it does not recognise are never printed.
import { parseArgs } from "node:util";
import { fileURLToPath } from "node:url";
import { backupKey, selectPrunable } from "./lib/d1_backup.mjs";

const nonNegativeInteger = (name, value) => {
  const parsed = Number(value);
  if (!Number.isInteger(parsed) || parsed < 0) {
    throw new Error(`--${name} must be a non-negative integer, got ${JSON.stringify(value)}`);
  }
  return parsed;
};

const dateOption = (name, value) => {
  if (value === undefined) {
    return new Date();
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    throw new Error(`--${name} must be an ISO date, got ${JSON.stringify(value)}`);
  }
  return date;
};

const readStdin = async () => {
  const chunks = [];
  for await (const chunk of process.stdin) {
    chunks.push(chunk);
  }
  return Buffer.concat(chunks).toString("utf8");
};

const DATABASE_NAME = /^[A-Za-z0-9_-]+$/;

const main = async (argv) => {
  const { positionals, values } = parseArgs({
    args: argv,
    allowPositionals: true,
    options: {
      prefix: { type: "string", default: "d1/" },
      database: { type: "string", default: "cfwdon" },
      at: { type: "string" },
      now: { type: "string" },
      "keep-days": { type: "string", default: "35" },
      "keep-months": { type: "string", default: "12" },
      "keep-latest": { type: "string", default: "7" },
    },
  });
  if (!DATABASE_NAME.test(values.database)) {
    throw new Error(
      `--database must match ${DATABASE_NAME}, got ${JSON.stringify(values.database)}`,
    );
  }
  const [command] = positionals;
  if (command === "key") {
    process.stdout.write(
      `${backupKey(values.prefix, values.database, dateOption("at", values.at))}\n`,
    );
    return;
  }
  if (command === "prune") {
    const keys = (await readStdin())
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter((line) => line !== "");
    const prunable = selectPrunable(keys, {
      prefix: values.prefix,
      database: values.database,
      now: dateOption("now", values.now),
      keepDays: nonNegativeInteger("keep-days", values["keep-days"]),
      keepMonths: nonNegativeInteger("keep-months", values["keep-months"]),
      keepLatest: Math.max(1, nonNegativeInteger("keep-latest", values["keep-latest"])),
    });
    process.stdout.write(prunable.map((key) => `${key}\n`).join(""));
    return;
  }
  throw new Error(`usage: d1_backup.mjs key|prune [options] (got ${JSON.stringify(command)})`);
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  });
}
