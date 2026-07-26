import type { BridgeConfig, Env } from "./types";

export function loadConfig(env: Env): BridgeConfig {
  const domain = required(env.INSTANCE_DOMAIN, "INSTANCE_DOMAIN").toLowerCase();
  const username = sanitizeUsername(
    required(env.ACTOR_USERNAME, "ACTOR_USERNAME"),
  );
  const base = `https://${domain}`;
  const actorId = `${base}/actors/${username}`;

  return {
    domain,
    username,
    displayName: env.ACTOR_NAME?.trim() || username,
    actorId,
    inboxUrl: `${actorId}/inbox`,
    outboxUrl: `${actorId}/outbox`,
    followersUrl: `${actorId}/followers`,
    followingUrl: `${actorId}/following`,
    preferredUsername: username,
    keyId: `${actorId}#main-key`,
    allowlist: parseAllowlist(env.ALLOWLIST_ACTOR_URIS || ""),
    appendSourceUrl: parseBool(env.APPEND_SOURCE_URL, true),
    maxTweetChars: parsePositiveInt(env.MAX_TWEET_CHARS, 280),
  };
}

function required(value: string | undefined, name: string): string {
  const trimmed = value?.trim();
  if (!trimmed) {
    throw new Error(`${name} is required`);
  }
  return trimmed;
}

function sanitizeUsername(value: string): string {
  const username = value.trim().toLowerCase();
  if (!/^[a-z0-9_]{1,64}$/.test(username)) {
    throw new Error("ACTOR_USERNAME must match /^[a-z0-9_]{1,64}$/");
  }
  return username;
}

export function parseAllowlist(raw: string): Set<string> {
  const uris = raw
    .split(/[,\n]/)
    .map((part) => part.trim())
    .filter(Boolean);
  return new Set(uris);
}

function parseBool(raw: string | undefined, fallback: boolean): boolean {
  if (raw == null || raw.trim() === "") {
    return fallback;
  }
  const value = raw.trim().toLowerCase();
  if (["1", "true", "yes", "on"].includes(value)) {
    return true;
  }
  if (["0", "false", "no", "off"].includes(value)) {
    return false;
  }
  return fallback;
}

function parsePositiveInt(raw: string | undefined, fallback: number): number {
  const parsed = Number.parseInt(raw ?? "", 10);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    return fallback;
  }
  return parsed;
}
