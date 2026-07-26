import { buildTweetText } from "./content";
import {
  getTweetIdForObject,
  putTweetIdForObject,
} from "./store";
import { PermanentXError, RetryableXError, createTweet } from "./x-client";
import type { BridgeConfig, Env, MirrorJob } from "../types";

export async function processMirrorJob(
  env: Env,
  config: BridgeConfig,
  job: MirrorJob,
): Promise<void> {
  const existing = await getTweetIdForObject(env.STORE, job.objectId);
  if (existing) {
    console.log("mirror job already completed", {
      objectId: job.objectId,
      tweetId: existing,
    });
    return;
  }

  if (!config.allowlist.has(job.attributedTo)) {
    console.warn("mirror job dropped; actor no longer allowlisted", {
      attributedTo: job.attributedTo,
      objectId: job.objectId,
    });
    return;
  }

  const text = buildTweetText({
    contentHtml: job.contentHtml,
    sourceUrl: job.sourceUrl,
    appendSourceUrl: config.appendSourceUrl,
    maxChars: config.maxTweetChars,
  });

  if (!text.trim()) {
    console.warn("mirror job dropped; empty tweet text", {
      objectId: job.objectId,
    });
    return;
  }

  try {
    const tweet = await createTweet(env, text);
    await putTweetIdForObject(env.STORE, job.objectId, tweet.id);
    console.log("mirrored to x", {
      objectId: job.objectId,
      tweetId: tweet.id,
      attributedTo: job.attributedTo,
    });
  } catch (error) {
    if (error instanceof PermanentXError) {
      console.error("permanent x post failure", {
        objectId: job.objectId,
        status: error.status,
        message: error.message,
      });
      return;
    }
    if (error instanceof RetryableXError) {
      console.error("retryable x post failure", {
        objectId: job.objectId,
        status: error.status,
        message: error.message,
      });
      throw error;
    }
    throw error;
  }
}
