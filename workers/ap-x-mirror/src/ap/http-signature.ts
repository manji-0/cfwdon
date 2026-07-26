import {
  importPublicKeyPem,
  parseHttpDateMs,
  sha256DigestHeader,
  signRsaSha256,
  verifyRsaSha256,
} from "./crypto";
import { asObject, asString } from "./documents";
import type { JsonObject } from "../types";

const MAX_DATE_SKEW_MS = 12 * 60 * 60 * 1000;
const REQUIRED_HEADERS = ["(request-target)", "date", "digest"] as const;
const ACTIVITYPUB_ACCEPT =
  'application/activity+json, application/ld+json; profile="https://www.w3.org/ns/activitystreams"';

export interface ParsedSignature {
  keyId: string;
  algorithm: string;
  headers: string[];
  signature: string;
}

export function parseSignatureHeader(header: string): ParsedSignature | null {
  const parts = new Map<string, string>();
  for (const segment of header.split(",")) {
    const match = segment.trim().match(/^([^=]+)="([^"]*)"$/);
    if (!match) {
      continue;
    }
    parts.set(match[1].trim(), match[2]);
  }

  const keyId = parts.get("keyId");
  const signature = parts.get("signature");
  const headersRaw = parts.get("headers");
  if (!keyId || !signature || !headersRaw) {
    return null;
  }

  return {
    keyId,
    algorithm: (parts.get("algorithm") || "rsa-sha256").toLowerCase(),
    headers: headersRaw.split(/\s+/).map((h) => h.toLowerCase()),
    signature,
  };
}

export function buildSigningString(
  method: string,
  pathAndQuery: string,
  headers: string[],
  headerValues: Record<string, string>,
): string {
  return headers
    .map((name) => {
      if (name === "(request-target)") {
        return `(request-target): ${method.toLowerCase()} ${pathAndQuery}`;
      }
      const value = headerValues[name];
      if (value == null) {
        throw new Error(`missing signed header value: ${name}`);
      }
      return `${name}: ${value}`;
    })
    .join("\n");
}

export async function verifyInboxSignature(
  request: Request,
  body: BufferSource,
): Promise<{ ok: true; keyId: string; actorUri: string } | { ok: false; reason: string }> {
  const signatureHeader = request.headers.get("Signature");
  if (!signatureHeader) {
    return { ok: false, reason: "missing Signature header" };
  }

  const parsed = parseSignatureHeader(signatureHeader);
  if (!parsed) {
    return { ok: false, reason: "invalid Signature header" };
  }
  if (
    parsed.algorithm &&
    parsed.algorithm !== "rsa-sha256" &&
    parsed.algorithm !== "hs2019"
  ) {
    return { ok: false, reason: `unsupported signature algorithm: ${parsed.algorithm}` };
  }
  for (const required of REQUIRED_HEADERS) {
    if (!parsed.headers.includes(required)) {
      return { ok: false, reason: `signature missing required header: ${required}` };
    }
  }

  const dateHeader = request.headers.get("Date");
  if (!dateHeader) {
    return { ok: false, reason: "missing Date header" };
  }
  const dateMs = parseHttpDateMs(dateHeader);
  if (dateMs == null || Math.abs(Date.now() - dateMs) > MAX_DATE_SKEW_MS) {
    return { ok: false, reason: "Date header outside allowed skew" };
  }

  const digestHeader = request.headers.get("Digest");
  if (!digestHeader) {
    return { ok: false, reason: "missing Digest header" };
  }
  const expectedDigest = await sha256DigestHeader(body);
  if (!digestEquals(digestHeader, expectedDigest)) {
    return { ok: false, reason: "Digest mismatch" };
  }

  const url = new URL(request.url);
  const pathAndQuery = `${url.pathname}${url.search}`;
  const hostHeader = request.headers.get("Host") || url.host;
  if (!hostMatches(hostHeader, url.hostname)) {
    return { ok: false, reason: "Host header mismatch" };
  }

  const headerValues: Record<string, string> = {
    host: hostHeader,
    date: dateHeader,
    digest: digestHeader,
  };
  const contentType = request.headers.get("Content-Type");
  if (contentType) {
    headerValues["content-type"] = contentType;
  }

  let signingString: string;
  try {
    signingString = buildSigningString(
      request.method,
      pathAndQuery,
      parsed.headers,
      headerValues,
    );
  } catch (error) {
    return {
      ok: false,
      reason: error instanceof Error ? error.message : "failed to build signing string",
    };
  }

  const publicKeyPem = await fetchPublicKeyPem(parsed.keyId);
  if (!publicKeyPem) {
    return { ok: false, reason: "failed to fetch signing public key" };
  }

  const publicKey = await importPublicKeyPem(publicKeyPem);
  const valid = await verifyRsaSha256(publicKey, signingString, parsed.signature);
  if (!valid) {
    return { ok: false, reason: "signature verification failed" };
  }

  const actorUri = parsed.keyId.split("#")[0] || parsed.keyId;
  return { ok: true, keyId: parsed.keyId, actorUri };
}

export async function signedActivityPubPost(options: {
  targetUrl: string;
  body: JsonObject;
  keyId: string;
  privateKey: CryptoKey;
}): Promise<Response> {
  const bodyText = JSON.stringify(options.body);
  const bodyBytes = new TextEncoder().encode(bodyText);
  const digest = await sha256DigestHeader(bodyBytes);
  const date = new Date().toUTCString();
  const url = new URL(options.targetUrl);
  const pathAndQuery = `${url.pathname}${url.search}`;
  const contentType = "application/activity+json";
  const headersList = [
    "(request-target)",
    "host",
    "date",
    "digest",
    "content-type",
  ];
  const signingString = buildSigningString("POST", pathAndQuery, headersList, {
    host: url.host,
    date,
    digest,
    "content-type": contentType,
  });
  const signature = await signRsaSha256(options.privateKey, signingString);

  return fetch(options.targetUrl, {
    method: "POST",
    headers: {
      Host: url.host,
      Date: date,
      Digest: digest,
      "Content-Type": contentType,
      Accept: ACTIVITYPUB_ACCEPT,
      Signature: `keyId="${options.keyId}",algorithm="rsa-sha256",headers="${headersList.join(" ")}",signature="${signature}"`,
    },
    body: bodyText,
    redirect: "manual",
  });
}

export async function fetchRemoteActor(actorUri: string): Promise<JsonObject | null> {
  const response = await fetch(actorUri, {
    method: "GET",
    headers: { Accept: ACTIVITYPUB_ACCEPT },
    redirect: "follow",
  });
  if (!response.ok) {
    return null;
  }
  try {
    const json = (await response.json()) as unknown;
    return asObject(json);
  } catch {
    return null;
  }
}

async function fetchPublicKeyPem(keyId: string): Promise<string | null> {
  const actorUri = keyId.split("#")[0] || keyId;
  const actor = await fetchRemoteActor(actorUri);
  if (!actor) {
    return null;
  }

  const publicKey = asObject(actor.publicKey);
  if (publicKey) {
    const id = asString(publicKey.id);
    if (id && id !== keyId && !keyId.startsWith(actorUri)) {
      // Still accept owner key when fragment differs but owner matches.
    }
    const pem = asString(publicKey.publicKeyPem);
    if (pem) {
      return pem;
    }
  }

  // Some implementations put the key under publicKeys / assertionMethod.
  return null;
}

function digestEquals(provided: string, expected: string): boolean {
  const normalizedProvided = provided
    .split(",")
    .map((part) => part.trim())
    .find((part) => part.toLowerCase().startsWith("sha-256="));
  if (!normalizedProvided) {
    return false;
  }
  return normalizedProvided.replace(/\s+/g, "") === expected.replace(/\s+/g, "");
}

function hostMatches(hostHeader: string, urlHost: string): boolean {
  const header = hostHeader.trim();
  if (header.toLowerCase() === urlHost.toLowerCase()) {
    return true;
  }
  const idx = header.lastIndexOf(":");
  if (idx > 0 && !header.includes("]")) {
    const name = header.slice(0, idx);
    const port = header.slice(idx + 1);
    if (/^\d+$/.test(port)) {
      return name.toLowerCase() === urlHost.toLowerCase();
    }
  }
  return false;
}
