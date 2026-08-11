import { errAsync, okAsync, type ResultAsync } from "neverthrow";
import {
  NotificationPolicy,
  type NotificationPolicyAction,
} from "@/domain/settings/notification-policy";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonFetchJson, mastodonPatchJson } from "@/infrastructure/http/mastodon-fetch";

export const fetchNotificationPolicy = (): ResultAsync<NotificationPolicy, MastodonFetchError> =>
  mastodonFetchJson("/api/v1/notifications/policy").andThen((raw) => {
    const parsed = NotificationPolicy.schema.safeParse(raw);
    if (!parsed.success) {
      return errAsync({ kind: "ValidationError" } as const);
    }
    return okAsync(parsed.data);
  });

export type UpdateNotificationPolicyInput = Readonly<{
  forNotFollowing?: NotificationPolicyAction;
  forNotFollowers?: NotificationPolicyAction;
  forNewAccounts?: NotificationPolicyAction;
  forPrivateMentions?: NotificationPolicyAction;
  forLimitedAccounts?: NotificationPolicyAction;
}>;

export const updateNotificationPolicy = (
  input: UpdateNotificationPolicyInput,
): ResultAsync<NotificationPolicy, MastodonFetchError> => {
  const body: Record<string, string> = {};
  if (input.forNotFollowing !== undefined) {
    body.for_not_following = input.forNotFollowing;
  }
  if (input.forNotFollowers !== undefined) {
    body.for_not_followers = input.forNotFollowers;
  }
  if (input.forNewAccounts !== undefined) {
    body.for_new_accounts = input.forNewAccounts;
  }
  if (input.forPrivateMentions !== undefined) {
    body.for_private_mentions = input.forPrivateMentions;
  }
  if (input.forLimitedAccounts !== undefined) {
    body.for_limited_accounts = input.forLimitedAccounts;
  }
  return mastodonPatchJson("/api/v1/notifications/policy", body).andThen((raw) => {
    const parsed = NotificationPolicy.schema.safeParse(raw);
    if (!parsed.success) {
      return errAsync({ kind: "ValidationError" } as const);
    }
    return okAsync(parsed.data);
  });
};
