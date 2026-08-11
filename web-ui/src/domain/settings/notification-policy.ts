import { z } from "zod";

export type NotificationPolicyAction = "accept" | "filter" | "drop";

export type NotificationPolicy = Readonly<{
  forNotFollowing: NotificationPolicyAction;
  forNotFollowers: NotificationPolicyAction;
  forNewAccounts: NotificationPolicyAction;
  forPrivateMentions: NotificationPolicyAction;
  forLimitedAccounts: NotificationPolicyAction;
}>;

const actionSchema = z.enum(["accept", "filter", "drop"]);

export const NotificationPolicy = {
  schema: z
    .object({
      for_not_following: actionSchema,
      for_not_followers: actionSchema,
      for_new_accounts: actionSchema,
      for_private_mentions: actionSchema,
      for_limited_accounts: actionSchema,
    })
    .transform(
      (value): NotificationPolicy => ({
        forNotFollowing: value.for_not_following,
        forNotFollowers: value.for_not_followers,
        forNewAccounts: value.for_new_accounts,
        forPrivateMentions: value.for_private_mentions,
        forLimitedAccounts: value.for_limited_accounts,
      }),
    ),

  actionLabel: (action: NotificationPolicyAction): string => {
    switch (action) {
      case "accept":
        return "受け取る";
      case "filter":
        return "リクエストにする";
      case "drop":
        return "破棄";
    }
  },
} as const;
