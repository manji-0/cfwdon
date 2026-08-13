import { type ResultAsync } from "neverthrow";
import type { TrendTag } from "@/domain/trends/trend";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonFetchJson } from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
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
