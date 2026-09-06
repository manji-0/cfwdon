import { type } from "arktype";
import { Notification } from "@/domain/notification/notification";
import type { Notification as NotificationState } from "@/domain/notification/notification";
import type { Status } from "@/domain/status/status";
import { parseAccountRef } from "@/infrastructure/mastodon/parsers/account";
import { parseStatus } from "@/infrastructure/mastodon/parsers/status";

const notificationFromPayload = (value: {
  readonly id: string;
  readonly type: string;
  readonly group_key: string;
  readonly created_at: string;
  readonly account: NotificationState["account"];
  readonly status?: Status;
}): NotificationState => {
  const fields = {
    id: value.id,
    groupKey: value.group_key,
    createdAt: value.created_at,
    account: value.account,
  };
  const status = value.status;
  const missingStatus = () =>
    Notification.unknown({ ...fields, type: value.type, status: null });
  switch (value.type) {
    case "mention":
      return status ? Notification.mention({ ...fields, status }) : missingStatus();
    case "status":
      return status ? Notification.posted({ ...fields, status }) : missingStatus();
    case "reblog":
      return status ? Notification.reblog({ ...fields, status }) : missingStatus();
    case "favourite":
      return status ? Notification.favourite({ ...fields, status }) : missingStatus();
    case "poll":
      return status ? Notification.poll({ ...fields, status }) : missingStatus();
    case "update":
      return status ? Notification.update({ ...fields, status }) : missingStatus();
    case "follow":
      return Notification.follow(fields);
    case "follow_request":
      return Notification.followRequest(fields);
    case "quote":
      return status ? Notification.quote({ ...fields, status }) : missingStatus();
    case "quoted_update":
      return status ? Notification.quotedUpdate({ ...fields, status }) : missingStatus();
    default:
      return Notification.unknown({ ...fields, type: value.type, status: status ?? null });
  }
};

const NotificationParser = type({
  id: "string>0",
  type: "string>0",
  group_key: "string",
  created_at: "string",
  account: parseAccountRef,
  "status?": parseStatus,
}).pipe((value) => notificationFromPayload(value));

export const parseNotification = NotificationParser;
export const parseNotificationList = type(NotificationParser, "[]");
