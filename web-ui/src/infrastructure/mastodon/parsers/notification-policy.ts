import { type } from "arktype";
import type { NotificationPolicy } from "@/domain/settings/notification-policy";

const PolicyAction = "'accept' | 'filter' | 'drop'";

export const parseNotificationPolicy = type({
  for_not_following: PolicyAction,
  for_not_followers: PolicyAction,
  for_new_accounts: PolicyAction,
  for_private_mentions: PolicyAction,
  for_limited_accounts: PolicyAction,
}).pipe(
  (value): NotificationPolicy => ({
    forNotFollowing: value.for_not_following,
    forNotFollowers: value.for_not_followers,
    forNewAccounts: value.for_new_accounts,
    forPrivateMentions: value.for_private_mentions,
    forLimitedAccounts: value.for_limited_accounts,
  }),
);
