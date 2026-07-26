import type { Env, MirrorJob } from "../types";

const ACTIVITY_TTL_SECONDS = 60 * 60 * 24 * 30;
const OBJECT_TTL_SECONDS = 60 * 60 * 24 * 365;

export async function claimActivity(
  store: KVNamespace,
  activityId: string,
): Promise<boolean> {
  const key = activityKey(activityId);
  const existing = await store.get(key);
  if (existing) {
    return false;
  }
  await store.put(key, "claimed", { expirationTtl: ACTIVITY_TTL_SECONDS });
  return true;
}

export async function getTweetIdForObject(
  store: KVNamespace,
  objectId: string,
): Promise<string | null> {
  return store.get(objectKey(objectId));
}

export async function putTweetIdForObject(
  store: KVNamespace,
  objectId: string,
  tweetId: string,
): Promise<void> {
  await store.put(objectKey(objectId), tweetId, {
    expirationTtl: OBJECT_TTL_SECONDS,
  });
}

export async function recordFollowState(
  store: KVNamespace,
  actorUri: string,
  state: "pending" | "accepted" | "rejected",
): Promise<void> {
  await store.put(followKey(actorUri), state);
}

export async function getFollowState(
  store: KVNamespace,
  actorUri: string,
): Promise<string | null> {
  return store.get(followKey(actorUri));
}

export async function enqueueMirrorJob(
  env: Env,
  job: MirrorJob,
): Promise<void> {
  await env.MIRROR_QUEUE.send(job);
}

function activityKey(activityId: string): string {
  return `activity:${activityId}`;
}

function objectKey(objectId: string): string {
  return `object:${objectId}`;
}

function followKey(actorUri: string): string {
  return `follow:${actorUri}`;
}
