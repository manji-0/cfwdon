import { type ResultAsync } from "neverthrow";
import type { PollDraft } from "@/domain/status/poll";
import type { ScheduledStatus } from "@/domain/status/scheduled";
import type { MastodonFetchError } from "@/domain/errors/http-error";
import type { PageQuery } from "@/domain/pagination";
import {
  mastodonDeleteJson,
  mastodonFetchJson,
  mastodonPostJson,
  pageParams,
} from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import {
  parseScheduledStatus,
  parseScheduledStatusList,
} from "@/infrastructure/mastodon/parsers/scheduled";

export type CreateScheduledStatusInput = Readonly<{
  text: string;
  visibility: string;
  scheduledAt: string;
  spoilerText?: string;
  sensitive?: boolean;
  language?: string | null;
  inReplyToId?: string;
  quotedStatusId?: string;
  mediaIds?: ReadonlyArray<string>;
  poll?: PollDraft | null;
}>;

export const createScheduledStatus = (
  input: CreateScheduledStatusInput,
): ResultAsync<ScheduledStatus, MastodonFetchError> => {
  const body: Record<string, unknown> = {
    status: input.text,
    visibility: input.visibility,
    scheduled_at: input.scheduledAt,
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
    parseMastodon(parseScheduledStatus, raw),
  );
};

export const fetchScheduledStatuses = (
  query: PageQuery = {},
): ResultAsync<ReadonlyArray<ScheduledStatus>, MastodonFetchError> => {
  const params = pageParams(query);
  return mastodonFetchJson(`/api/v1/scheduled_statuses?${params}`).andThen((raw) =>
    parseMastodon(parseScheduledStatusList, raw),
  );
};

export const cancelScheduledStatus = (
  scheduledId: string,
): ResultAsync<null, MastodonFetchError> =>
  mastodonDeleteJson(`/api/v1/scheduled_statuses/${encodeURIComponent(scheduledId)}`).map(
    () => null,
  );
