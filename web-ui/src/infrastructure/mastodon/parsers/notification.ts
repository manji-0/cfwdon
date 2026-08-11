import { type } from "arktype";
import type { Notification } from "@/domain/notification/notification";
import { parseAccountRef } from "@/infrastructure/mastodon/parsers/account";
import { parseStatus } from "@/infrastructure/mastodon/parsers/status";

const NotificationParser = type({
  id: "string>0",
  type: "string>0",
  group_key: "string",
  created_at: "string",
  account: parseAccountRef,
  "status?": parseStatus,
}).pipe(
  (value): Notification => ({
    id: value.id,
    type: value.type,
    groupKey: value.group_key,
    createdAt: value.created_at,
    account: value.account,
    status: value.status ?? null,
  }),
);

export const parseNotification = NotificationParser;
export const parseNotificationList = type(NotificationParser, "[]");
