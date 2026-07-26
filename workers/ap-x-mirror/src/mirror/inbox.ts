import {
  activityHasType,
  activityObjectId,
  asObject,
  asString,
  attributedToUri,
  isMastodonPublicNote,
  objectContentHtml,
  objectSourceUrl,
} from "../ap/documents";
import { verifyInboxSignature } from "../ap/http-signature";
import type { BridgeConfig, Env, JsonObject, MirrorJob } from "../types";
import {
  claimActivity,
  enqueueMirrorJob,
  getTweetIdForObject,
  recordFollowState,
} from "./store";

export async function handleInboxPost(
  request: Request,
  env: Env,
  config: BridgeConfig,
): Promise<Response> {
  const body = await request.arrayBuffer();
  const signature = await verifyInboxSignature(request, body);
  if (!signature.ok) {
    console.warn("inbox signature rejected", { reason: signature.reason });
    return new Response("Unauthorized", { status: 401 });
  }

  let activity: JsonObject;
  try {
    activity = JSON.parse(new TextDecoder().decode(body)) as JsonObject;
  } catch {
    return new Response("Invalid JSON", { status: 400 });
  }

  if (activityHasType(activity, "Accept")) {
    await handleAccept(env, config, activity, signature.actorUri);
    return accepted();
  }

  if (activityHasType(activity, "Reject")) {
    await handleReject(env, activity, signature.actorUri);
    return accepted();
  }

  if (!activityHasType(activity, "Create")) {
    return accepted();
  }

  const object = asObject(activity.object);
  if (!object || !activityHasType(object, "Note")) {
    return accepted();
  }

  // Public only: Public must be in `to` (unlisted / followers / DM are skipped).
  if (!isMastodonPublicNote(object, activity)) {
    return accepted();
  }

  const attributedTo = attributedToUri(object, activity);
  if (!attributedTo || !config.allowlist.has(attributedTo)) {
    return accepted();
  }

  // Signed actor must match the note author for allowlisted mirrors.
  if (signature.actorUri !== attributedTo) {
    console.warn("inbox attribution mismatch", {
      signedActor: signature.actorUri,
      attributedTo,
    });
    return accepted();
  }

  const activityId = asString(activity.id);
  const objectId = activityObjectId(object) || asString(object.id);
  if (!activityId || !objectId) {
    return accepted();
  }

  const claimed = await claimActivity(env.STORE, activityId);
  if (!claimed) {
    return accepted();
  }

  const existingTweet = await getTweetIdForObject(env.STORE, objectId);
  if (existingTweet) {
    return accepted();
  }

  const job: MirrorJob = {
    activityId,
    objectId,
    contentHtml: objectContentHtml(object),
    sourceUrl: objectSourceUrl(object),
    attributedTo,
  };

  await enqueueMirrorJob(env, job);
  console.log("mirrored create enqueued", {
    activityId,
    objectId,
    attributedTo,
  });
  return accepted();
}

async function handleAccept(
  env: Env,
  config: BridgeConfig,
  activity: JsonObject,
  signedActor: string,
): Promise<void> {
  const object = asObject(activity.object);
  if (!object || !activityHasType(object, "Follow")) {
    return;
  }
  const objectActor = asString(object.actor);
  if (objectActor !== config.actorId) {
    return;
  }
  if (!config.allowlist.has(signedActor)) {
    return;
  }
  await recordFollowState(env.STORE, signedActor, "accepted");
  console.log("follow accepted", { actor: signedActor });
}

async function handleReject(
  env: Env,
  activity: JsonObject,
  signedActor: string,
): Promise<void> {
  const object = asObject(activity.object);
  if (!object || !activityHasType(object, "Follow")) {
    return;
  }
  await recordFollowState(env.STORE, signedActor, "rejected");
  console.log("follow rejected", { actor: signedActor });
}

function accepted(): Response {
  return new Response(null, { status: 202 });
}
