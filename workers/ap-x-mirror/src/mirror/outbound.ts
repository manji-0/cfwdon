import { fetchRemoteActor, signedActivityPubPost } from "../ap/http-signature";
import { asString } from "../ap/documents";
import type { BridgeConfig, Env, JsonObject } from "../types";
import { getFollowState, recordFollowState } from "./store";

export async function followAllowlistedActors(
  env: Env,
  config: BridgeConfig,
  privateKey: CryptoKey,
): Promise<{ attempted: number; results: FollowResult[] }> {
  const results: FollowResult[] = [];
  for (const actorUri of config.allowlist) {
    const existing = await getFollowState(env.STORE, actorUri);
    if (existing === "accepted" || existing === "pending") {
      results.push({ actorUri, status: "skipped", detail: existing });
      continue;
    }

    try {
      const result = await sendFollow(env, config, privateKey, actorUri);
      results.push(result);
    } catch (error) {
      results.push({
        actorUri,
        status: "error",
        detail: error instanceof Error ? error.message : String(error),
      });
    }
  }
  return { attempted: config.allowlist.size, results };
}

interface FollowResult {
  actorUri: string;
  status: "sent" | "skipped" | "error";
  detail?: string;
  httpStatus?: number;
}

async function sendFollow(
  env: Env,
  config: BridgeConfig,
  privateKey: CryptoKey,
  actorUri: string,
): Promise<FollowResult> {
  const actor = await fetchRemoteActor(actorUri);
  if (!actor) {
    return { actorUri, status: "error", detail: "failed to fetch actor" };
  }

  const sharedInbox =
    typeof actor.endpoints === "object" &&
    actor.endpoints &&
    !Array.isArray(actor.endpoints)
      ? asString((actor.endpoints as JsonObject).sharedInbox)
      : null;
  const inbox = sharedInbox || asString(actor.inbox);

  if (!inbox) {
    return { actorUri, status: "error", detail: "remote actor missing inbox" };
  }

  const activityId = `${config.actorId}/activities/follow/${crypto.randomUUID()}`;
  const activity: JsonObject = {
    "@context": "https://www.w3.org/ns/activitystreams",
    id: activityId,
    type: "Follow",
    actor: config.actorId,
    object: actorUri,
  };

  const response = await signedActivityPubPost({
    targetUrl: inbox,
    body: activity,
    keyId: config.keyId,
    privateKey,
  });

  if (response.status >= 200 && response.status < 300) {
    await recordFollowState(env.STORE, actorUri, "pending");
    return {
      actorUri,
      status: "sent",
      httpStatus: response.status,
    };
  }

  const body = await response.text();
  return {
    actorUri,
    status: "error",
    httpStatus: response.status,
    detail: body.slice(0, 300),
  };
}
