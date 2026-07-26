const PEM_PRIVATE_HEADER = "-----BEGIN PRIVATE KEY-----";
const PEM_PRIVATE_FOOTER = "-----END PRIVATE KEY-----";
const PEM_PUBLIC_HEADER = "-----BEGIN PUBLIC KEY-----";
const PEM_PUBLIC_FOOTER = "-----END PUBLIC KEY-----";

export async function importActorPrivateKey(
  pem: string,
): Promise<CryptoKey> {
  const der = pemToArrayBuffer(pem, PEM_PRIVATE_HEADER, PEM_PRIVATE_FOOTER);
  return crypto.subtle.importKey(
    "pkcs8",
    der,
    { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" },
    true,
    ["sign"],
  );
}

export async function publicKeyPemFromPrivate(
  privateKey: CryptoKey,
): Promise<string> {
  const jwk = (await crypto.subtle.exportKey("jwk", privateKey)) as JsonWebKey;
  if (!jwk.n || !jwk.e) {
    throw new Error("private key JWK is missing RSA public components");
  }
  const publicKey = await crypto.subtle.importKey(
    "jwk",
    { kty: "RSA", n: jwk.n, e: jwk.e, alg: "RS256", ext: true, key_ops: ["verify"] },
    { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" },
    true,
    ["verify"],
  );
  const spki = (await crypto.subtle.exportKey("spki", publicKey)) as ArrayBuffer;
  return arrayBufferToPem(spki, PEM_PUBLIC_HEADER, PEM_PUBLIC_FOOTER);
}

export async function importPublicKeyPem(pem: string): Promise<CryptoKey> {
  const der = pemToArrayBuffer(pem, PEM_PUBLIC_HEADER, PEM_PUBLIC_FOOTER);
  return crypto.subtle.importKey(
    "spki",
    der,
    { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" },
    false,
    ["verify"],
  );
}

export async function sha256DigestHeader(body: BufferSource): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", body);
  return `SHA-256=${arrayBufferToBase64(digest)}`;
}

export async function signRsaSha256(
  privateKey: CryptoKey,
  data: string,
): Promise<string> {
  const signature = await crypto.subtle.sign(
    "RSASSA-PKCS1-v1_5",
    privateKey,
    new TextEncoder().encode(data),
  );
  return arrayBufferToBase64(signature);
}

export async function verifyRsaSha256(
  publicKey: CryptoKey,
  data: string,
  signatureB64: string,
): Promise<boolean> {
  try {
    return await crypto.subtle.verify(
      "RSASSA-PKCS1-v1_5",
      publicKey,
      base64ToArrayBuffer(signatureB64),
      new TextEncoder().encode(data),
    );
  } catch {
    return false;
  }
}

export function nowHttpDate(): string {
  return new Date().toUTCString();
}

export function parseHttpDateMs(value: string): number | null {
  const ms = Date.parse(value);
  return Number.isFinite(ms) ? ms : null;
}

function pemToArrayBuffer(
  pem: string,
  header: string,
  footer: string,
): ArrayBuffer {
  const normalized = pem.replace(/\r/g, "").trim();
  if (!normalized.includes(header) || !normalized.includes(footer)) {
    throw new Error("PEM key is missing expected headers");
  }
  const body = normalized
    .replace(header, "")
    .replace(footer, "")
    .replace(/\s+/g, "");
  return base64ToArrayBuffer(body);
}

function arrayBufferToPem(
  buffer: ArrayBuffer,
  header: string,
  footer: string,
): string {
  const b64 = arrayBufferToBase64(buffer);
  const lines = b64.match(/.{1,64}/g) ?? [b64];
  return `${header}\n${lines.join("\n")}\n${footer}\n`;
}

export function arrayBufferToBase64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  let binary = "";
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary);
}

export function base64ToArrayBuffer(b64: string): ArrayBuffer {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes.buffer;
}

export async function hmacSha1Base64(
  key: string,
  data: string,
): Promise<string> {
  const cryptoKey = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(key),
    { name: "HMAC", hash: "SHA-1" },
    false,
    ["sign"],
  );
  const signature = await crypto.subtle.sign(
    "HMAC",
    cryptoKey,
    new TextEncoder().encode(data),
  );
  return arrayBufferToBase64(signature);
}
