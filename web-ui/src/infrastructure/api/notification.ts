import { errAsync, okAsync, ResultAsync } from "neverthrow";
import { NotificationListSchema } from "@/domain/notification/notification";
import type { Notification } from "@/domain/notification/notification";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonFetchJson } from "@/infrastructure/http/mastodon-fetch";

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
  return mastodonFetchJson(`/api/v1/notifications?${params}`).andThen((raw) => {
    const parsed = NotificationListSchema.safeParse(raw);
    if (!parsed.success) {
      return errAsync({ kind: "ValidationError" } as const);
    }
    return okAsync(parsed.data);
  });
};
