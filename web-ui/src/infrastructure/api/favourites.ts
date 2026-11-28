import { type ResultAsync } from "neverthrow";
import type { Status } from "@/domain/status/status";
import type { MastodonFetchError } from "@/domain/errors/http-error";
import type { Page, PageQuery } from "@/domain/pagination";
import { mastodonFetchPage, pageParams } from "@/infrastructure/http/mastodon-fetch";
import { parseStatusList } from "@/infrastructure/mastodon/parsers/status";

export const fetchFavourites = (
  query: PageQuery = {},
): ResultAsync<Page<Status>, MastodonFetchError> => {
  const params = pageParams(query);
  return mastodonFetchPage(`/api/v1/favourites?${params}`, parseStatusList);
};
