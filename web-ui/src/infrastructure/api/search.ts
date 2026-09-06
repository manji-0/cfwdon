import { type ResultAsync } from "neverthrow";
import type { SearchResults } from "@/domain/search/search";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonFetchJson } from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import { parseSearchResults } from "@/infrastructure/mastodon/parsers/search";

export type SearchQuery = Readonly<{
  q: string;
  type?: "accounts" | "statuses" | "hashtags";
  limit?: number;
  offset?: number;
  resolve?: boolean;
}>;

export const search = (query: SearchQuery): ResultAsync<SearchResults, MastodonFetchError> => {
  const params = new URLSearchParams();
  params.set("q", query.q);
  params.set("limit", String(query.limit ?? 20));
  if (query.type) {
    params.set("type", query.type);
  }
  if (query.offset !== undefined) {
    params.set("offset", String(query.offset));
  }
  if (query.resolve) {
    params.set("resolve", "true");
  }
  return mastodonFetchJson(`/api/v2/search?${params}`).andThen((raw) =>
    parseMastodon(parseSearchResults, raw),
  );
};
