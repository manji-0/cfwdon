export interface Env {
  STORE: KVNamespace;
  MIRROR_QUEUE: Queue<MirrorJob>;

  INSTANCE_DOMAIN: string;
  ACTOR_USERNAME: string;
  ACTOR_NAME: string;
  ALLOWLIST_ACTOR_URIS: string;
  APPEND_SOURCE_URL: string;
  MAX_TWEET_CHARS: string;

  /** PKCS#8 PEM RSA private key for the bridge actor. */
  ACTOR_PRIVATE_KEY_PEM: string;
  /** Shared bearer token for /admin/* bootstrap routes. */
  ADMIN_TOKEN: string;

  X_API_KEY: string;
  X_API_SECRET: string;
  X_ACCESS_TOKEN: string;
  X_ACCESS_TOKEN_SECRET: string;
}

export interface MirrorJob {
  activityId: string;
  objectId: string;
  contentHtml: string;
  sourceUrl: string;
  attributedTo: string;
}

export interface BridgeConfig {
  domain: string;
  username: string;
  displayName: string;
  actorId: string;
  inboxUrl: string;
  outboxUrl: string;
  followersUrl: string;
  followingUrl: string;
  preferredUsername: string;
  keyId: string;
  allowlist: Set<string>;
  appendSourceUrl: boolean;
  maxTweetChars: number;
}

export type JsonObject = Record<string, unknown>;
