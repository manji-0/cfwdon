import type { AccountRef } from "@/domain/account/account";
import type { Status } from "@/domain/status/status";

type NotificationMeta = Readonly<{
  id: string;
  groupKey: string;
  createdAt: string;
  account: AccountRef;
}>;

export type MentionNotification = NotificationMeta &
  Readonly<{
    kind: "Mention";
    status: Status;
  }>;

export type PostedNotification = NotificationMeta &
  Readonly<{
    kind: "Posted";
    status: Status;
  }>;

export type ReblogNotification = NotificationMeta &
  Readonly<{
    kind: "Reblog";
    status: Status;
  }>;

export type FavouriteNotification = NotificationMeta &
  Readonly<{
    kind: "Favourite";
    status: Status;
  }>;

export type PollNotification = NotificationMeta &
  Readonly<{
    kind: "Poll";
    status: Status;
  }>;

export type UpdateNotification = NotificationMeta &
  Readonly<{
    kind: "Update";
    status: Status;
  }>;

export type FollowNotification = NotificationMeta &
  Readonly<{
    kind: "Follow";
  }>;

export type FollowRequestNotification = NotificationMeta &
  Readonly<{
    kind: "FollowRequest";
  }>;

export type QuoteNotification = NotificationMeta &
  Readonly<{
    kind: "Quote";
    status: Status;
  }>;

export type QuotedUpdateNotification = NotificationMeta &
  Readonly<{
    kind: "QuotedUpdate";
    status: Status;
  }>;

export type UnknownNotification = NotificationMeta &
  Readonly<{
    kind: "Unknown";
    type: string;
    status: Status | null;
  }>;

export type Notification =
  | MentionNotification
  | PostedNotification
  | ReblogNotification
  | FavouriteNotification
  | PollNotification
  | UpdateNotification
  | FollowNotification
  | FollowRequestNotification
  | QuoteNotification
  | QuotedUpdateNotification
  | UnknownNotification;

export const Notification = {
  mention: (fields: NotificationMeta & Readonly<{ status: Status }>): MentionNotification => ({
    kind: "Mention",
    ...fields,
  }),

  posted: (fields: NotificationMeta & Readonly<{ status: Status }>): PostedNotification => ({
    kind: "Posted",
    ...fields,
  }),

  reblog: (fields: NotificationMeta & Readonly<{ status: Status }>): ReblogNotification => ({
    kind: "Reblog",
    ...fields,
  }),

  favourite: (fields: NotificationMeta & Readonly<{ status: Status }>): FavouriteNotification => ({
    kind: "Favourite",
    ...fields,
  }),

  poll: (fields: NotificationMeta & Readonly<{ status: Status }>): PollNotification => ({
    kind: "Poll",
    ...fields,
  }),

  update: (fields: NotificationMeta & Readonly<{ status: Status }>): UpdateNotification => ({
    kind: "Update",
    ...fields,
  }),

  follow: (fields: NotificationMeta): FollowNotification => ({
    kind: "Follow",
    ...fields,
  }),

  followRequest: (fields: NotificationMeta): FollowRequestNotification => ({
    kind: "FollowRequest",
    ...fields,
  }),

  quote: (fields: NotificationMeta & Readonly<{ status: Status }>): QuoteNotification => ({
    kind: "Quote",
    ...fields,
  }),

  quotedUpdate: (
    fields: NotificationMeta & Readonly<{ status: Status }>,
  ): QuotedUpdateNotification => ({
    kind: "QuotedUpdate",
    ...fields,
  }),

  unknown: (
    fields: NotificationMeta & Readonly<{ type: string; status: Status | null }>,
  ): UnknownNotification => ({
    kind: "Unknown",
    ...fields,
  }),

  status: (notification: Notification): Status | null => {
    switch (notification.kind) {
      case "Follow":
      case "FollowRequest":
        return null;
      case "Unknown":
        return notification.status;
      case "Mention":
      case "Posted":
      case "Reblog":
      case "Favourite":
      case "Poll":
      case "Update":
      case "Quote":
      case "QuotedUpdate":
        return notification.status;
    }
  },

  withStatus: (notification: Notification, status: Status): Notification => {
    switch (notification.kind) {
      case "Follow":
      case "FollowRequest":
        return notification;
      case "Unknown":
      case "Mention":
      case "Posted":
      case "Reblog":
      case "Favourite":
      case "Poll":
      case "Update":
      case "Quote":
      case "QuotedUpdate":
        return { ...notification, status };
    }
  },

  label: (notification: Notification): string => {
    const name = notification.account.displayName || notification.account.username;
    switch (notification.kind) {
      case "Mention":
        return `${name} が返信しました`;
      case "Posted":
        return `${name} が投稿しました`;
      case "Reblog":
        return `${name} がブーストしました`;
      case "Follow":
        return `${name} がフォローしました`;
      case "FollowRequest":
        return `${name} がフォローリクエストを送りました`;
      case "Favourite":
        return `${name} がいいねしました`;
      case "Poll":
        return `${name} の投票が終了しました`;
      case "Update":
        return `${name} が投稿を編集しました`;
      case "Quote":
        return `${name} が引用しました`;
      case "QuotedUpdate":
        return `${name} が引用元を編集しました`;
      case "Unknown":
        return `${name} から通知`;
    }
  },
} as const;
