import { type ResultAsync } from "neverthrow";
import { BookmarkList } from "@/domain/bookmarks/bookmark";
import { notImplemented, type MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";

/** TODO(Phase 5): GET `/api/v1/bookmarks` with cursor pagination. */
export const fetchBookmarks = (): ResultAsync<BookmarkList, MastodonFetchError> =>
  notImplemented("bookmarks");
