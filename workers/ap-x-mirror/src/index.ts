import { loadConfig } from "./config";
import {
  actorDocument,
  emptyOrderedCollection,
  webfingerResource,
} from "./ap/documents";
import {
  importActorPrivateKey,
  publicKeyPemFromPrivate,
} from "./ap/crypto";
import { handleInboxPost } from "./mirror/inbox";
import { followAllowlistedActors } from "./mirror/outbound";
import { processMirrorJob } from "./mirror/process";
import type { BridgeConfig, Env, MirrorJob } from "./types";

const AP_CONTENT_TYPE = 'application/activity+json; charset=utf-8';
const JSON_CONTENT_TYPE = "application/json; charset=utf-8";
const JRD_CONTENT_TYPE = "application/jrd+json; charset=utf-8";

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    try {
      return await handleFetch(request, env);
    } catch (error) {
      console.error("unhandled fetch error", {
        message: error instanceof Error ? error.message : String(error),
      });
      return json({ error: "internal error" }, 500);
    }
  },

  async queue(
    batch: MessageBatch<MirrorJob>,
    env: Env,
  ): Promise<void> {
    const config = loadConfig(env);
    for (const message of batch.messages) {
      try {
        await processMirrorJob(env, config, message.body);
        message.ack();
      } catch (error) {
        console.error("mirror queue message failed", {
          objectId: message.body.objectId,
          message: error instanceof Error ? error.message : String(error),
        });
        message.retry();
      }
    }
  },
};

async function handleFetch(request: Request, env: Env): Promise<Response> {
  const url = new URL(request.url);
  const config = loadConfig(env);
  const username = config.username;
  const actorBase = `/actors/${username}`;

  if (request.method === "GET" && url.pathname === "/health") {
    return json({
      ok: true,
      actor: config.actorId,
      allowlistSize: config.allowlist.size,
    });
  }

  if (
    request.method === "GET" &&
    (url.pathname === "/.well-known/webfinger" ||
      url.pathname === "/.well-known/webfinger/")
  ) {
    const resource = url.searchParams.get("resource");
    if (!resource) {
      return json({ error: "missing resource" }, 400);
    }
    const document = webfingerResource(config, resource);
    if (!document) {
      return new Response("Not found", { status: 404 });
    }
    return new Response(JSON.stringify(document), {
      status: 200,
      headers: { "Content-Type": JRD_CONTENT_TYPE },
    });
  }

  if (request.method === "GET" && url.pathname === actorBase) {
    const publicKeyPem = await loadPublicKeyPem(env);
    return activityJson(actorDocument(config, publicKeyPem));
  }

  if (request.method === "GET" && url.pathname === `${actorBase}/outbox`) {
    return activityJson(emptyOrderedCollection(config.outboxUrl));
  }
  if (request.method === "GET" && url.pathname === `${actorBase}/followers`) {
    return activityJson(emptyOrderedCollection(config.followersUrl));
  }
  if (request.method === "GET" && url.pathname === `${actorBase}/following`) {
    return activityJson(emptyOrderedCollection(config.followingUrl));
  }

  if (request.method === "POST" && url.pathname === `${actorBase}/inbox`) {
    return handleInboxPost(request, env, config);
  }

  if (request.method === "POST" && url.pathname === "/admin/follow-allowlist") {
    if (!authorizeAdmin(request, env)) {
      return json({ error: "unauthorized" }, 401);
    }
    const privateKey = await importActorPrivateKey(
      requireSecret(env.ACTOR_PRIVATE_KEY_PEM, "ACTOR_PRIVATE_KEY_PEM"),
    );
    const result = await followAllowlistedActors(env, config, privateKey);
    return json({ ok: true, ...result });
  }

  if (request.method === "GET" && url.pathname === "/") {
    return json({
      service: "cfwdon-ap-x-mirror",
      actor: config.actorId,
      webfinger: `acct:${config.preferredUsername}@${config.domain}`,
    });
  }

  return json({ error: "not found" }, 404);
}

async function loadPublicKeyPem(env: Env): Promise<string> {
  const privateKey = await importActorPrivateKey(
    requireSecret(env.ACTOR_PRIVATE_KEY_PEM, "ACTOR_PRIVATE_KEY_PEM"),
  );
  return publicKeyPemFromPrivate(privateKey);
}

function authorizeAdmin(request: Request, env: Env): boolean {
  const token = env.ADMIN_TOKEN?.trim();
  if (!token) {
    return false;
  }
  const auth = request.headers.get("Authorization") || "";
  return auth === `Bearer ${token}`;
}

function requireSecret(value: string | undefined, name: string): string {
  const trimmed = value?.trim();
  if (!trimmed) {
    throw new Error(`${name} is required`);
  }
  return trimmed;
}

function activityJson(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": AP_CONTENT_TYPE },
  });
}

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": JSON_CONTENT_TYPE },
  });
}

// Keep BridgeConfig referenced for wrangler type narrowing in editors.
export type { BridgeConfig, Env, MirrorJob };
