import { hmacSha1Base64 } from "../ap/crypto";
import type { Env } from "../types";

export class PermanentXError extends Error {
  readonly status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = "PermanentXError";
    this.status = status;
  }
}

export class RetryableXError extends Error {
  readonly status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = "RetryableXError";
    this.status = status;
  }
}

export async function createTweet(
  env: Env,
  text: string,
): Promise<{ id: string }> {
  const url = "https://api.x.com/2/tweets";
  const body = JSON.stringify({ text });
  const authorization = await buildOAuth1Header({
    method: "POST",
    url,
    consumerKey: requireSecret(env.X_API_KEY, "X_API_KEY"),
    consumerSecret: requireSecret(env.X_API_SECRET, "X_API_SECRET"),
    token: requireSecret(env.X_ACCESS_TOKEN, "X_ACCESS_TOKEN"),
    tokenSecret: requireSecret(env.X_ACCESS_TOKEN_SECRET, "X_ACCESS_TOKEN_SECRET"),
  });

  const response = await fetch(url, {
    method: "POST",
    headers: {
      Authorization: authorization,
      "Content-Type": "application/json",
    },
    body,
  });

  const responseText = await response.text();
  if (response.ok) {
    const parsed = JSON.parse(responseText) as { data?: { id?: string } };
    const id = parsed.data?.id;
    if (!id) {
      throw new RetryableXError(response.status, "X API response missing tweet id");
    }
    return { id };
  }

  const message = `X API error ${response.status}: ${responseText.slice(0, 500)}`;
  if (response.status === 429 || response.status >= 500) {
    throw new RetryableXError(response.status, message);
  }
  throw new PermanentXError(response.status, message);
}

async function buildOAuth1Header(options: {
  method: string;
  url: string;
  consumerKey: string;
  consumerSecret: string;
  token: string;
  tokenSecret: string;
}): Promise<string> {
  const oauth: Record<string, string> = {
    oauth_consumer_key: options.consumerKey,
    oauth_nonce: randomNonce(),
    oauth_signature_method: "HMAC-SHA1",
    oauth_timestamp: Math.floor(Date.now() / 1000).toString(),
    oauth_token: options.token,
    oauth_version: "1.0",
  };

  const paramString = Object.keys(oauth)
    .sort()
    .map((key) => `${percentEncode(key)}=${percentEncode(oauth[key])}`)
    .join("&");

  const baseString = [
    options.method.toUpperCase(),
    percentEncode(normalizeBaseUrl(options.url)),
    percentEncode(paramString),
  ].join("&");

  const signingKey = `${percentEncode(options.consumerSecret)}&${percentEncode(options.tokenSecret)}`;
  oauth.oauth_signature = await hmacSha1Base64(signingKey, baseString);

  const header = Object.keys(oauth)
    .sort()
    .map((key) => `${percentEncode(key)}="${percentEncode(oauth[key])}"`)
    .join(", ");

  return `OAuth ${header}`;
}

function normalizeBaseUrl(url: string): string {
  const parsed = new URL(url);
  parsed.hash = "";
  parsed.search = "";
  return parsed.toString();
}

function percentEncode(value: string): string {
  return encodeURIComponent(value).replace(/[!'()*]/g, (char) => {
    return `%${char.charCodeAt(0).toString(16).toUpperCase()}`;
  });
}

function randomNonce(): string {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

function requireSecret(value: string | undefined, name: string): string {
  const trimmed = value?.trim();
  if (!trimmed) {
    throw new PermanentXError(500, `${name} secret is not configured`);
  }
  return trimmed;
}
