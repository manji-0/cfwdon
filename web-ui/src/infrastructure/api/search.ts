import { errAsync, okAsync, ResultAsync } from "neverthrow";
import { SearchResults } from "@/domain/search/search";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonFetchJson } from "@/infrastructure/http/mastodon-fetch";

export type SearchQuery = Readonly<{
  q: string;
  type?: "accounts" | "statuses" | "hashtags";
  limit?: number;
  offset?: number;
}>;

export const search = (
  query: SearchQuery,
): ResultAsync<SearchResults, MastodonFetchError> => {
  const trimmed = query.q.trim();
  if (!trimmed) {
    return okAsync({ accounts: [], statuses: [], hashtags: [] });
  }
  const params = new URLSearchParams();
  params.set("q", trimmed);
  params.set("limit", String(query.limit ?? 20));
  if (query.type) {
    params.set("type", query.type);
  }
  if (query.offset) {
    params.set("offset", String(query.offset));
  }
  return mastodonFetchJson(`/api/v2/search?${params}`).andThen((raw) => {
    const parsed = SearchResults.schema.safeParse(raw);
    if (!parsed.success) {
      return errAsync({ kind: "ValidationError" } as const);
    }
    return okAsync(parsed.data);
  });
};
