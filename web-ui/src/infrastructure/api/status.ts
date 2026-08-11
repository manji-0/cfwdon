import { errAsync, okAsync, ResultAsync } from "neverthrow";
import type { Status, StatusContext } from "@/domain/status/status";
import { Status as StatusModel, StatusContext as StatusContextModel, StatusListSchema } from "@/domain/status/status";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonFetchJson, mastodonPostJson } from "@/infrastructure/http/mastodon-fetch";

export type TimelineQuery = Readonly<{
  maxId?: string;
  limit?: number;
}>;

export const fetchHomeTimeline = (
  query: TimelineQuery = {},
): ResultAsync<ReadonlyArray<Status>, MastodonFetchError> => {
  const params = new URLSearchParams();
  params.set("limit", String(query.limit ?? 20));
  if (query.maxId) {
    params.set("max_id", query.maxId);
  }
  return mastodonFetchJson(`/api/v1/timelines/home?${params}`).andThen((raw) => {
    const parsed = StatusListSchema.safeParse(raw);
    if (!parsed.success) {
      return errAsync({ kind: "ValidationError" } as const);
    }
    return okAsync(parsed.data);
  });
};

export const fetchStatusContext = (
  statusId: string,
): ResultAsync<StatusContext, MastodonFetchError> =>
  mastodonFetchJson(`/api/v1/statuses/${encodeURIComponent(statusId)}/context`).andThen((raw) => {
    const parsed = StatusContextModel.schema.safeParse(raw);
    if (!parsed.success) {
      return errAsync({ kind: "ValidationError" } as const);
    }
    return okAsync(parsed.data);
  });

export const fetchStatus = (statusId: string): ResultAsync<Status, MastodonFetchError> =>
  mastodonFetchJson(`/api/v1/statuses/${encodeURIComponent(statusId)}`).andThen((raw) => {
    const parsed = StatusModel.schema.safeParse(raw);
    if (!parsed.success) {
      return errAsync({ kind: "ValidationError" } as const);
    }
    return okAsync(parsed.data);
  });

export type CreateStatusInput = Readonly<{
  text: string;
  visibility: string;
  spoilerText?: string;
  sensitive?: boolean;
  inReplyToId?: string;
}>;

export const createStatus = (
  input: CreateStatusInput,
): ResultAsync<Status, MastodonFetchError> =>
  mastodonPostJson("/api/v1/statuses", {
    status: input.text,
    visibility: input.visibility,
    spoiler_text: input.spoilerText ?? "",
    sensitive: input.sensitive ?? false,
    in_reply_to_id: input.inReplyToId,
  }).andThen((raw) => {
    const parsed = StatusModel.schema.safeParse(raw);
    if (!parsed.success) {
      return errAsync({ kind: "ValidationError" } as const);
    }
    return okAsync(parsed.data);
  });

export const favouriteStatus = (statusId: string): ResultAsync<Status, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/statuses/${encodeURIComponent(statusId)}/favourite`, {}).andThen((raw) => {
    const parsed = StatusModel.schema.safeParse(raw);
    if (!parsed.success) {
      return errAsync({ kind: "ValidationError" } as const);
    }
    return okAsync(parsed.data);
  });

export const unfavouriteStatus = (statusId: string): ResultAsync<Status, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/statuses/${encodeURIComponent(statusId)}/unfavourite`, {}).andThen((raw) => {
    const parsed = StatusModel.schema.safeParse(raw);
    if (!parsed.success) {
      return errAsync({ kind: "ValidationError" } as const);
    }
    return okAsync(parsed.data);
  });

export const reblogStatus = (statusId: string): ResultAsync<Status, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/statuses/${encodeURIComponent(statusId)}/reblog`, {}).andThen((raw) => {
    const parsed = StatusModel.schema.safeParse(raw);
    if (!parsed.success) {
      return errAsync({ kind: "ValidationError" } as const);
    }
    return okAsync(parsed.data);
  });

export const unreblogStatus = (statusId: string): ResultAsync<Status, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/statuses/${encodeURIComponent(statusId)}/unreblog`, {}).andThen((raw) => {
    const parsed = StatusModel.schema.safeParse(raw);
    if (!parsed.success) {
      return errAsync({ kind: "ValidationError" } as const);
    }
    return okAsync(parsed.data);
  });
