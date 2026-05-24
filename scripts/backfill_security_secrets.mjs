#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { createCipheriv, createHash, randomBytes } from "node:crypto";

const args = new Set(process.argv.slice(2));
const database = valueAfter("--database") ?? "DB";
const remoteMode = args.has("--local") ? "--local" : "--remote";
const dryRun = args.has("--dry-run");
const encryptionKey = process.env.ACCOUNT_PRIVATE_KEY_ENCRYPTION_KEY;

function valueAfter(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function sqlString(value) {
  return `'${String(value).replaceAll("'", "''")}'`;
}

function tokenHash(token) {
  return `sha256:${createHash("sha256").update(token).digest("hex")}`;
}

function encryptSecret(plaintext) {
  if (!encryptionKey) {
    throw new Error("ACCOUNT_PRIVATE_KEY_ENCRYPTION_KEY is required to encrypt account private keys");
  }
  const key = createHash("sha256").update(encryptionKey).digest();
  const iv = randomBytes(12);
  const cipher = createCipheriv("aes-256-gcm", key, iv);
  const ciphertext = Buffer.concat([
    cipher.update(plaintext, "utf8"),
    cipher.final(),
    cipher.getAuthTag(),
  ]);
  return `v1:${iv.toString("base64")}:${ciphertext.toString("base64")}`;
}

function wrangler(command) {
  const result = spawnSync(
    "wrangler",
    ["d1", "execute", database, remoteMode, "--json", "--command", command],
    { encoding: "utf8" },
  );
  if (result.status !== 0) {
    throw new Error(result.stderr || result.stdout || `wrangler exited with ${result.status}`);
  }
  return JSON.parse(result.stdout);
}

function resultRows(output) {
  const first = Array.isArray(output) ? output[0] : output;
  return first?.results ?? first?.result?.[0]?.results ?? [];
}

function executeUpdates(statements) {
  if (statements.length === 0) {
    return;
  }
  const command = statements.join(";\n");
  if (dryRun) {
    console.log(command);
    return;
  }
  wrangler(command);
}

function backfillTokenTable(table) {
  const rows = resultRows(
    wrangler(
      `SELECT access_token FROM ${table} WHERE access_token IS NOT NULL AND access_token NOT LIKE 'sha256:%'`,
    ),
  );
  const statements = rows.map((row) => {
    const hash = tokenHash(row.access_token);
    return `UPDATE ${table} SET access_token = ${sqlString(hash)}, access_token_hash = ${sqlString(hash)} WHERE access_token = ${sqlString(row.access_token)}`;
  });
  executeUpdates(statements);
  console.log(`${dryRun ? "planned" : "backfilled"} ${statements.length} ${table} token row(s)`);
}

function backfillAccountPrivateKeys() {
  const rows = resultRows(
    wrangler(
      "SELECT id, private_key_jwk FROM accounts WHERE private_key_jwk IS NOT NULL AND private_key_jwk != ''",
    ),
  );
  const statements = rows.flatMap((row) => {
    const encrypted = row.private_key_jwk.startsWith("v1:")
      ? row.private_key_jwk
      : encryptSecret(row.private_key_jwk);
    return [
      `INSERT INTO account_private_keys (account_id, private_key_jwk_encrypted, created_at, updated_at) VALUES (${sqlString(row.id)}, ${sqlString(encrypted)}, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP) ON CONFLICT(account_id) DO UPDATE SET private_key_jwk_encrypted = excluded.private_key_jwk_encrypted, updated_at = CURRENT_TIMESTAMP`,
      `UPDATE accounts SET private_key_jwk = '', updated_at = CURRENT_TIMESTAMP WHERE id = ${sqlString(row.id)}`,
    ];
  });
  executeUpdates(statements);
  console.log(`${dryRun ? "planned" : "backfilled"} ${rows.length} account private key row(s)`);
}

backfillTokenTable("oauth_access_tokens");
backfillTokenTable("oauth_app_access_tokens");
backfillAccountPrivateKeys();
