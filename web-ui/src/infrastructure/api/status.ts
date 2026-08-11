import { type ResultAsync } from "neverthrow";
import type { Status, StatusContext } from "@/domain/status/status";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonFetchJson, mastodonPostJson } from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import {
  parseStatus,
  parseStatusContext,
  parseStatusList,
} from "@/infrastructure/mastodon/parsers/status";

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
  return mastodonFetchJson(`/api/v1/timelines/home?${params}`).andThen((raw) =>
    parseMastodon(parseStatusList, raw),
  );
};

export const fetchStatusContext = (
  statusId: string,
): ResultAsync<StatusContext, MastodonFetchError> =>
  mastodonFetchJson(`/api/v1/statuses/${encodeURIComponent(statusId)}/context`).andThen((raw) =>
    parseMastodon(parseStatusContext, raw),
  );

export const fetchStatus = (statusId: string): ResultAsync<Status, MastodonFetchError> =>
  mastodonFetchJson(`/api/v1/statuses/${encodeURIComponent(statusId)}`).andThen((raw) =>
    parseMastodon(parseStatus, raw),
  );

export type CreateStatusInput = Readonly<{
  text: string;
  visibility: string;
  spoilerText?: string;
  sensitive?: boolean;
  inReplyToId?: string;
  mediaIds?: ReadonlyArray<string>;
}>;

export const createStatus = (
  input: CreateStatusInput,
): ResultAsync<Status, MastodonFetchError> => {
  const body: Record<string, unknown> = {
    status: input.text,
    visibility: input.visibility,
    spoiler_text: input.spoilerText ?? "",
    sensitive: input.sensitive ?? false,
    in_reply_to_id: input.inReplyToId,
  };
  if (input.mediaIds && input.mediaIds.length > 0) {
    body.media_ids = input.mediaIds;
  }
  return mastodonPostJson("/api/v1/statuses", body).andThen((raw) =>
    parseMastodon(parseStatus, raw),
  );
};

const mapStatusResponse = (raw: unknown): ResultAsync<Status, MastodonFetchError> =>
  parseMastodon(parseStatus, raw);

export const favouriteStatus = (statusId: string): ResultAsync<Status, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/statuses/${encodeURIComponent(statusId)}/favourite`, {}).andThen(
    mapStatusResponse,
  );

export const unfavouriteStatus = (statusId: string): ResultAsync<Status, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/statuses/${encodeURIComponent(statusId)}/unfavourite`, {}).andThen(
    mapStatusResponse,
  );

export const reblogStatus = (statusId: string): ResultAsync<Status, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/statuses/${encodeURIComponent(statusId)}/reblog`, {}).andThen(
    mapStatusResponse,
  );

export const unreblogStatus = (statusId: string): ResultAsync<Status, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/statuses/${encodeURIComponent(statusId)}/unreblog`, {}).andThen(
    mapStatusResponse,
  );
