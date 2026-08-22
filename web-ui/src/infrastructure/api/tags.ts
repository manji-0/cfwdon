import { type ResultAsync } from "neverthrow";
import type { FollowedTag } from "@/domain/tags/followed-tag";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import {
  mastodonFetchJson,
  mastodonPostJson,
} from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import { parseFollowedTag, parseFollowedTagList } from "@/infrastructure/mastodon/parsers/tag";

export const fetchFollowedTags = (): ResultAsync<
  ReadonlyArray<FollowedTag>,
  MastodonFetchError
> =>
  mastodonFetchJson("/api/v1/followed_tags").andThen((raw) =>
    parseMastodon(parseFollowedTagList, raw),
  );

export const followTag = (name: string): ResultAsync<FollowedTag, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/tags/${encodeURIComponent(name)}/follow`, {}).andThen((raw) =>
    parseMastodon(parseFollowedTag, raw),
  );

export const unfollowTag = (name: string): ResultAsync<FollowedTag, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/tags/${encodeURIComponent(name)}/unfollow`, {}).andThen((raw) =>
    parseMastodon(parseFollowedTag, raw),
  );
