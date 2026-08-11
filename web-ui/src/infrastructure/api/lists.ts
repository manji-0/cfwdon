import { type ResultAsync } from "neverthrow";
import type { AccountList } from "@/domain/lists/list";
import type { Status } from "@/domain/status/status";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonFetchJson } from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import { parseAccountListCollection } from "@/infrastructure/mastodon/parsers/lists";
import { parseStatusList } from "@/infrastructure/mastodon/parsers/status";

export const fetchLists = (): ResultAsync<ReadonlyArray<AccountList>, MastodonFetchError> =>
  mastodonFetchJson("/api/v1/lists").andThen((raw) =>
    parseMastodon(parseAccountListCollection, raw),
  );

export type ListTimelineQuery = Readonly<{
  maxId?: string;
  limit?: number;
}>;

export const fetchListTimeline = (
  listId: string,
  query: ListTimelineQuery = {},
): ResultAsync<ReadonlyArray<Status>, MastodonFetchError> => {
  const params = new URLSearchParams();
  params.set("limit", String(query.limit ?? 20));
  if (query.maxId) {
    params.set("max_id", query.maxId);
  }
  return mastodonFetchJson(
    `/api/v1/timelines/list/${encodeURIComponent(listId)}?${params}`,
  ).andThen((raw) => parseMastodon(parseStatusList, raw));
};
