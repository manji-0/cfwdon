import { type ResultAsync } from "neverthrow";
import type { BookmarkList } from "@/domain/bookmarks/bookmark";
import type { MastodonFetchError } from "@/domain/errors/http-error";
import type { PageQuery } from "@/domain/pagination";
import { mastodonFetchJson, pageParams } from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import { parseStatusList } from "@/infrastructure/mastodon/parsers/status";

export const fetchBookmarks = (
  query: PageQuery = {},
): ResultAsync<BookmarkList, MastodonFetchError> => {
  const params = pageParams(query);
  return mastodonFetchJson(`/api/v1/bookmarks?${params}`).andThen((raw) =>
    parseMastodon(parseStatusList, raw),
  );
};
