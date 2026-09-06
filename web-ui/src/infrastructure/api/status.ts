import { okAsync, type ResultAsync } from "neverthrow";
import type { AccountProfile } from "@/domain/account/account";
import type { PollDraft } from "@/domain/status/poll";
import type { StatusEdit } from "@/domain/status/edit";
import type { StatusSource } from "@/domain/status/source";
import type { Status, StatusContext } from "@/domain/status/status";
import type { StatusTranslation } from "@/domain/status/translation";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import {
  mastodonDeleteJson,
  mastodonFetchJson,
  mastodonPostJson,
  mastodonPutJson,
} from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import {
  parseStatusEditList,
  parseStatusSource,
  parseStatusTranslation,
} from "@/infrastructure/mastodon/parsers/status-extra";
import {
  parseStatus,
  parseStatusContext,
  parseStatusList,
} from "@/infrastructure/mastodon/parsers/status";
import { parseAccountProfileList } from "@/infrastructure/mastodon/parsers/account";

export type TimelineQuery = Readonly<{
  maxId?: string;
  limit?: number;
}>;

const timelineParams = (query: TimelineQuery): string => {
  const params = new URLSearchParams();
  params.set("limit", String(query.limit ?? 20));
  if (query.maxId) {
    params.set("max_id", query.maxId);
  }
  return params.toString();
};

export const fetchHomeTimeline = (
  query: TimelineQuery = {},
): ResultAsync<ReadonlyArray<Status>, MastodonFetchError> =>
  mastodonFetchJson(`/api/v1/timelines/home?${timelineParams(query)}`).andThen((raw) =>
    parseMastodon(parseStatusList, raw),
  );

export const fetchPublicTimeline = (
  query: TimelineQuery & { local?: boolean } = {},
): ResultAsync<ReadonlyArray<Status>, MastodonFetchError> => {
  const params = new URLSearchParams(timelineParams(query));
  if (query.local) {
    params.set("local", "true");
  }
  return mastodonFetchJson(`/api/v1/timelines/public?${params}`).andThen((raw) =>
    parseMastodon(parseStatusList, raw),
  );
};

export const fetchTagTimeline = (
  tag: string,
  query: TimelineQuery = {},
): ResultAsync<ReadonlyArray<Status>, MastodonFetchError> =>
  mastodonFetchJson(
    `/api/v1/timelines/tag/${encodeURIComponent(tag)}?${timelineParams(query)}`,
  ).andThen((raw) => parseMastodon(parseStatusList, raw));

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
  language?: string | null;
  inReplyToId?: string;
  quotedStatusId?: string;
  mediaIds?: ReadonlyArray<string>;
  poll?: PollDraft | null;
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
    quoted_status_id: input.quotedStatusId,
  };
  if (input.language) {
    body.language = input.language;
  }
  if (input.mediaIds && input.mediaIds.length > 0) {
    body.media_ids = input.mediaIds;
  }
  if (input.poll) {
    body.poll = {
      options: input.poll.options
        .map((option) => option.trim())
        .filter((option) => option.length > 0),
      expires_in: input.poll.expiresIn,
      multiple: input.poll.multiple,
    };
  }
  return mastodonPostJson("/api/v1/statuses", body).andThen((raw) =>
    parseMastodon(parseStatus, raw),
  );
};

export const deleteStatus = (
  statusId: string,
): ResultAsync<Status | null, MastodonFetchError> =>
  mastodonDeleteJson(`/api/v1/statuses/${encodeURIComponent(statusId)}`).andThen((raw) => {
    if (raw === null) {
      return okAsync(null);
    }
    return parseMastodon(parseStatus, raw);
  });

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

export const bookmarkStatus = (statusId: string): ResultAsync<Status, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/statuses/${encodeURIComponent(statusId)}/bookmark`, {}).andThen(
    mapStatusResponse,
  );

export const unbookmarkStatus = (statusId: string): ResultAsync<Status, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/statuses/${encodeURIComponent(statusId)}/unbookmark`, {}).andThen(
    mapStatusResponse,
  );

export type UpdateStatusInput = Readonly<{
  text: string;
  spoilerText?: string;
  sensitive?: boolean;
  mediaIds?: ReadonlyArray<string>;
}>;

export const updateStatus = (
  statusId: string,
  input: UpdateStatusInput,
): ResultAsync<Status, MastodonFetchError> => {
  const body: Record<string, unknown> = {
    status: input.text,
    spoiler_text: input.spoilerText ?? "",
    sensitive: input.sensitive ?? false,
  };
  if (input.mediaIds && input.mediaIds.length > 0) {
    body.media_ids = input.mediaIds;
  }
  return mastodonPutJson(`/api/v1/statuses/${encodeURIComponent(statusId)}`, body).andThen(
    mapStatusResponse,
  );
};

export const fetchStatusSource = (
  statusId: string,
): ResultAsync<StatusSource, MastodonFetchError> =>
  mastodonFetchJson(`/api/v1/statuses/${encodeURIComponent(statusId)}/source`).andThen((raw) =>
    parseMastodon(parseStatusSource, raw),
  );

export const fetchStatusHistory = (
  statusId: string,
): ResultAsync<ReadonlyArray<StatusEdit>, MastodonFetchError> =>
  mastodonFetchJson(`/api/v1/statuses/${encodeURIComponent(statusId)}/history`).andThen((raw) =>
    parseMastodon(parseStatusEditList, raw),
  );

export const pinStatus = (statusId: string): ResultAsync<Status, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/statuses/${encodeURIComponent(statusId)}/pin`, {}).andThen(
    mapStatusResponse,
  );

export const unpinStatus = (statusId: string): ResultAsync<Status, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/statuses/${encodeURIComponent(statusId)}/unpin`, {}).andThen(
    mapStatusResponse,
  );

export const translateStatus = (
  statusId: string,
): ResultAsync<StatusTranslation, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/statuses/${encodeURIComponent(statusId)}/translate`, {}).andThen(
    (raw) => parseMastodon(parseStatusTranslation, raw),
  );

export const muteConversation = (statusId: string): ResultAsync<Status, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/statuses/${encodeURIComponent(statusId)}/mute`, {}).andThen(
    mapStatusResponse,
  );

export const unmuteConversation = (statusId: string): ResultAsync<Status, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/statuses/${encodeURIComponent(statusId)}/unmute`, {}).andThen(
    mapStatusResponse,
  );

export const fetchStatusFavouritedBy = (
  statusId: string,
  query: TimelineQuery = {},
): ResultAsync<ReadonlyArray<AccountProfile>, MastodonFetchError> =>
  mastodonFetchJson(
    `/api/v1/statuses/${encodeURIComponent(statusId)}/favourited_by?${timelineParams(query)}`,
  ).andThen((raw) => parseMastodon(parseAccountProfileList, raw));

export const fetchStatusRebloggedBy = (
  statusId: string,
  query: TimelineQuery = {},
): ResultAsync<ReadonlyArray<AccountProfile>, MastodonFetchError> =>
  mastodonFetchJson(
    `/api/v1/statuses/${encodeURIComponent(statusId)}/reblogged_by?${timelineParams(query)}`,
  ).andThen((raw) => parseMastodon(parseAccountProfileList, raw));

export const fetchStatusQuotes = (
  statusId: string,
  query: TimelineQuery = {},
): ResultAsync<ReadonlyArray<Status>, MastodonFetchError> =>
  mastodonFetchJson(
    `/api/v1/statuses/${encodeURIComponent(statusId)}/quotes?${timelineParams(query)}`,
  ).andThen((raw) => parseMastodon(parseStatusList, raw));
