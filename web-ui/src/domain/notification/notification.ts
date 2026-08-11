import { z } from "zod";
import { AccountRef } from "@/domain/account/account";
import { Status } from "@/domain/status/status";

export type Notification = Readonly<{
  id: string;
  type: string;
  groupKey: string;
  createdAt: string;
  account: AccountRef;
  status: Status | null;
}>;

const NotificationSchema = z
  .object({
    id: z.string().min(1),
    type: z.string().min(1),
    group_key: z.string(),
    created_at: z.string(),
    account: AccountRef.schema,
    status: Status.schema.nullable().optional(),
  })
  .transform(
    (value): Notification => ({
      id: value.id,
      type: value.type,
      groupKey: value.group_key,
      createdAt: value.created_at,
      account: value.account,
      status: value.status ?? null,
    }),
  );

export const NotificationListSchema = z.array(NotificationSchema);

export const NotificationModel = {
  label: (notification: Notification): string => {
    const name = notification.account.displayName || notification.account.username;
    switch (notification.type) {
      case "mention":
        return `${name} が返信しました`;
      case "status":
        return `${name} が投稿しました`;
      case "reblog":
        return `${name} がブーストしました`;
      case "follow":
        return `${name} がフォローしました`;
      case "follow_request":
        return `${name} がフォローリクエストを送りました`;
      case "favourite":
        return `${name} がいいねしました`;
      case "poll":
        return `${name} の投票が終了しました`;
      case "update":
        return `${name} が投稿を編集しました`;
      default:
        return `${name} から通知`;
    }
  },
} as const;
