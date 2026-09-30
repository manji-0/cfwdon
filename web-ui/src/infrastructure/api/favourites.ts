import { type ResultAsync } from "neverthrow";
import type { Status } from "@/domain/status/status";
import type { MastodonFetchError } from "@/domain/errors/http-error";
import type { PageQuery } from "@/domain/pagination";
import { mastodonFetchJson, pageParams } from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import { parseStatusList } from "@/infrastructure/mastodon/parsers/status";

export const fetchFavourites = (
  query: PageQuery = {},
): ResultAsync<ReadonlyArray<Status>, MastodonFetchError> => {
  const params = pageParams(query);
  return mastodonFetchJson(`/api/v1/favourites?${params}`).andThen((raw) =>
    parseMastodon(parseStatusList, raw),
  );
};
