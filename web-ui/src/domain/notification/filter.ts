export const NotificationFilter = {
  values: [
    "mention",
    "status",
    "reblog",
    "follow",
    "follow_request",
    "favourite",
    "poll",
    "update",
  ] as const,

  labels: {
    mention: "返信",
    status: "投稿",
    reblog: "ブースト",
    follow: "フォロー",
    follow_request: "リクエスト",
    favourite: "いいね",
    poll: "投票",
    update: "編集",
  } as const,
} as const;

export type NotificationFilterType = (typeof NotificationFilter.values)[number];
