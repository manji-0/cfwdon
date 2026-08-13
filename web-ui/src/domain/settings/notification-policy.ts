export type NotificationPolicyAction = "accept" | "filter" | "drop";

export type NotificationPolicy = Readonly<{
  forNotFollowing: NotificationPolicyAction;
  forNotFollowers: NotificationPolicyAction;
  forNewAccounts: NotificationPolicyAction;
  forPrivateMentions: NotificationPolicyAction;
  forLimitedAccounts: NotificationPolicyAction;
}>;

export const NotificationPolicy = {
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
