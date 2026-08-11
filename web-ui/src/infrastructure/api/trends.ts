import { errAsync, okAsync, type ResultAsync } from "neverthrow";
import { TrendTag, TrendTagModel } from "@/domain/trends/trend";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonFetchJson } from "@/infrastructure/http/mastodon-fetch";

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
  return mastodonFetchJson(`/api/v1/trends/tags?${params}`).andThen((raw) => {
    const parsed = TrendTagModel.listSchema.safeParse(raw);
    if (!parsed.success) {
      return errAsync({ kind: "ValidationError" } as const);
    }
    return okAsync(parsed.data);
  });
};
