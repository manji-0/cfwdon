import { type ResultAsync } from "neverthrow";
import type { Notification } from "@/domain/notification/notification";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import {
  mastodonFetchJson,
  mastodonPostJson,
} from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import { parseNotificationList } from "@/infrastructure/mastodon/parsers/notification";
import { type } from "arktype";

export type NotificationsQuery = Readonly<{
  maxId?: string;
  limit?: number;
  types?: ReadonlyArray<string>;
}>;

export const fetchNotifications = (
  query: NotificationsQuery = {},
): ResultAsync<ReadonlyArray<Notification>, MastodonFetchError> => {
  const params = new URLSearchParams();
  params.set("limit", String(query.limit ?? 20));
  if (query.maxId) {
    params.set("max_id", query.maxId);
  }
  for (const typeName of query.types ?? []) {
    params.append("types[]", typeName);
  }
  return mastodonFetchJson(`/api/v1/notifications?${params}`).andThen((raw) =>
    parseMastodon(parseNotificationList, raw),
  );
};

const UnreadCountParser = type({
  count: "number",
});

export const fetchUnreadNotificationCount = (): ResultAsync<number, MastodonFetchError> =>
  mastodonFetchJson("/api/v1/notifications/unread_count").andThen((raw) =>
    parseMastodon(UnreadCountParser, raw).map((value) => value.count),
  );

export const clearNotifications = (): ResultAsync<void, MastodonFetchError> =>
  mastodonPostJson("/api/v1/notifications/clear", {}).map(() => undefined);

export const dismissNotification = (notificationId: string): ResultAsync<void, MastodonFetchError> =>
  mastodonPostJson(
    `/api/v1/notifications/${encodeURIComponent(notificationId)}/dismiss`,
    {},
  ).map(() => undefined);
