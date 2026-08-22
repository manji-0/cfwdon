import { type ResultAsync } from "neverthrow";
import type { Status } from "@/domain/status/status";
import type { TrendLink } from "@/domain/trends/link";
import type { TrendTag } from "@/domain/trends/trend";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonFetchJson } from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import { parseStatusList } from "@/infrastructure/mastodon/parsers/status";
import { parseTrendLinkList } from "@/infrastructure/mastodon/parsers/trend-link";
import { parseTrendTagList } from "@/infrastructure/mastodon/parsers/trends";

export type TrendingTagsQuery = Readonly<{
  limit?: number;
  offset?: number;
}>;

export const fetchTrendingTags = (
  query: TrendingTagsQuery = {},
): ResultAsync<ReadonlyArray<TrendTag>, MastodonFetchError> => {
  const params = new URLSearchParams();
  params.set("limit", String(query.limit ?? 10));
  if (query.offset !== undefined) {
    params.set("offset", String(query.offset));
  }
  return mastodonFetchJson(`/api/v1/trends/tags?${params}`).andThen((raw) =>
    parseMastodon(parseTrendTagList, raw),
  );
};

export const fetchTrendingStatuses = (
  query: TrendingTagsQuery = {},
): ResultAsync<ReadonlyArray<Status>, MastodonFetchError> => {
  const params = new URLSearchParams();
  params.set("limit", String(query.limit ?? 10));
  if (query.offset !== undefined) {
    params.set("offset", String(query.offset));
  }
  return mastodonFetchJson(`/api/v1/trends/statuses?${params}`).andThen((raw) =>
    parseMastodon(parseStatusList, raw),
  );
};

export const fetchTrendingLinks = (
  query: TrendingTagsQuery = {},
): ResultAsync<ReadonlyArray<TrendLink>, MastodonFetchError> => {
  const params = new URLSearchParams();
  params.set("limit", String(query.limit ?? 10));
  if (query.offset !== undefined) {
    params.set("offset", String(query.offset));
  }
  return mastodonFetchJson(`/api/v1/trends/links?${params}`).andThen((raw) =>
    parseMastodon(parseTrendLinkList, raw),
  );
};
