import { type ResultAsync } from "neverthrow";
import type { Notification } from "@/domain/notification/notification";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonFetchJson } from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import { parseNotificationList } from "@/infrastructure/mastodon/parsers/notification";

export type NotificationsQuery = Readonly<{
  maxId?: string;
  limit?: number;
}>;

export const fetchNotifications = (
  query: NotificationsQuery = {},
): ResultAsync<ReadonlyArray<Notification>, MastodonFetchError> => {
  const params = new URLSearchParams();
  params.set("limit", String(query.limit ?? 20));
  if (query.maxId) {
    params.set("max_id", query.maxId);
  }
  return mastodonFetchJson(`/api/v1/notifications?${params}`).andThen((raw) =>
    parseMastodon(parseNotificationList, raw),
  );
};
