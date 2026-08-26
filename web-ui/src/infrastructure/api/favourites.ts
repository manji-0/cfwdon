import { type ResultAsync } from "neverthrow";
import type { Status } from "@/domain/status/status";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonFetchJson } from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import { parseStatusList } from "@/infrastructure/mastodon/parsers/status";

export type FavouritesQuery = Readonly<{
  maxId?: string;
  limit?: number;
}>;

export const fetchFavourites = (
  query: FavouritesQuery = {},
): ResultAsync<ReadonlyArray<Status>, MastodonFetchError> => {
  const params = new URLSearchParams();
  params.set("limit", String(query.limit ?? 20));
  if (query.maxId) {
    params.set("max_id", query.maxId);
  }
  return mastodonFetchJson(`/api/v1/favourites?${params}`).andThen((raw) =>
    parseMastodon(parseStatusList, raw),
  );
};
