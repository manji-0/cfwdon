import type { BridgeConfig, JsonObject } from "../types";

const AS_CONTEXT = "https://www.w3.org/ns/activitystreams";
const SECURITY_CONTEXT = "https://w3id.org/security/v1";

export function actorDocument(
  config: BridgeConfig,
  publicKeyPem: string,
): JsonObject {
  return {
    "@context": [AS_CONTEXT, SECURITY_CONTEXT],
    id: config.actorId,
    type: "Person",
    preferredUsername: config.preferredUsername,
    name: config.displayName,
    inbox: config.inboxUrl,
    outbox: config.outboxUrl,
    followers: config.followersUrl,
    following: config.followingUrl,
    url: config.actorId,
    manuallyApprovesFollowers: true,
    discoverable: false,
    indexable: false,
    publicKey: {
      id: config.keyId,
      owner: config.actorId,
      publicKeyPem,
    },
  };
}

export function webfingerResource(
  config: BridgeConfig,
  resource: string,
): JsonObject | null {
  const acct = `acct:${config.preferredUsername}@${config.domain}`;
  const actor = config.actorId;
  const normalized = resource.trim();
  if (
    normalized !== acct &&
    normalized.toLowerCase() !== acct.toLowerCase() &&
    normalized !== actor
  ) {
    return null;
  }

  return {
    subject: acct,
    aliases: [actor],
    links: [
      {
        rel: "self",
        type: "application/activity+json",
        href: actor,
      },
      {
        rel: "http://webfinger.net/rel/profile-page",
        type: "text/html",
        href: actor,
      },
    ],
  };
}

export function emptyOrderedCollection(id: string): JsonObject {
  return {
    "@context": AS_CONTEXT,
    id,
    type: "OrderedCollection",
    totalItems: 0,
    orderedItems: [],
  };
}

export function activityHasType(
  value: JsonObject,
  expected: string,
): boolean {
  const type = value.type;
  if (typeof type === "string") {
    return type === expected;
  }
  if (Array.isArray(type)) {
    return type.some((entry) => entry === expected);
  }
  return false;
}

export function asObject(value: unknown): JsonObject | null {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as JsonObject;
  }
  return null;
}

export function asString(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

export function activityObjectId(object: unknown): string | null {
  if (typeof object === "string") {
    return object.trim() || null;
  }
  const obj = asObject(object);
  return obj ? asString(obj.id) : null;
}

export function attributedToUri(
  object: JsonObject,
  activity: JsonObject,
): string | null {
  const fromObject = firstUri(object.attributedTo);
  if (fromObject) {
    return fromObject;
  }
  return firstUri(activity.actor);
}

function firstUri(value: unknown): string | null {
  if (typeof value === "string") {
    return value.trim() || null;
  }
  if (Array.isArray(value)) {
    for (const entry of value) {
      const uri = firstUri(entry);
      if (uri) {
        return uri;
      }
    }
    return null;
  }
  const obj = asObject(value);
  return obj ? asString(obj.id) : null;
}

export function objectSourceUrl(object: JsonObject): string {
  return (
    asString(object.url) ||
    asString(object.id) ||
    ""
  );
}

export function objectContentHtml(object: JsonObject): string {
  return asString(object.content) || asString(object.summary) || "";
}

const ACTIVITYSTREAMS_PUBLIC =
  "https://www.w3.org/ns/activitystreams#Public";
const ACTIVITYSTREAMS_PUBLIC_SHORT = "as:Public";

/** Mastodon public: Public appears in `to` (not only `cc` / unlisted). */
export function isMastodonPublicNote(
  object: JsonObject,
  activity: JsonObject,
): boolean {
  return (
    audienceContainsPublic(object.to) || audienceContainsPublic(activity.to)
  );
}

function audienceContainsPublic(value: unknown): boolean {
  for (const entry of flattenAudience(value)) {
    if (isPublicAudienceUri(entry)) {
      return true;
    }
  }
  return false;
}

function isPublicAudienceUri(value: string): boolean {
  return (
    value === ACTIVITYSTREAMS_PUBLIC ||
    value === ACTIVITYSTREAMS_PUBLIC_SHORT ||
    value === "Public"
  );
}

function flattenAudience(value: unknown): string[] {
  if (typeof value === "string") {
    const trimmed = value.trim();
    return trimmed ? [trimmed] : [];
  }
  if (Array.isArray(value)) {
    const out: string[] = [];
    for (const entry of value) {
      out.push(...flattenAudience(entry));
    }
    return out;
  }
  const obj = asObject(value);
  if (obj) {
    const id = asString(obj.id);
    return id ? [id] : [];
  }
  return [];
}
