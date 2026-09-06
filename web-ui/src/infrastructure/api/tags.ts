import { type ResultAsync } from "neverthrow";
import type { FeaturedTag } from "@/domain/tags/featured-tag";
import type { FollowedTag } from "@/domain/tags/followed-tag";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import {
  mastodonDeleteJson,
  mastodonFetchJson,
  mastodonPostJson,
} from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import {
  parseFeaturedTag,
  parseFeaturedTagList,
  parseFeaturedTagSuggestionList,
} from "@/infrastructure/mastodon/parsers/featured-tag";
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

export const fetchFeaturedTags = (): ResultAsync<
  ReadonlyArray<FeaturedTag>,
  MastodonFetchError
> =>
  mastodonFetchJson("/api/v1/featured_tags").andThen((raw) =>
    parseMastodon(parseFeaturedTagList, raw),
  );

export const fetchAccountFeaturedTags = (
  accountId: string,
): ResultAsync<ReadonlyArray<FeaturedTag>, MastodonFetchError> =>
  mastodonFetchJson(`/api/v1/accounts/${encodeURIComponent(accountId)}/featured_tags`).andThen(
    (raw) => parseMastodon(parseFeaturedTagList, raw),
  );

export const fetchFeaturedTagSuggestions = (): ResultAsync<
  ReadonlyArray<FeaturedTag>,
  MastodonFetchError
> =>
  mastodonFetchJson("/api/v1/featured_tags/suggestions").andThen((raw) =>
    parseMastodon(parseFeaturedTagSuggestionList, raw),
  );

export const featureTag = (name: string): ResultAsync<FeaturedTag, MastodonFetchError> =>
  mastodonPostJson("/api/v1/featured_tags", { name }).andThen((raw) =>
    parseMastodon(parseFeaturedTag, raw),
  );

export const unfeatureTag = (id: string): ResultAsync<void, MastodonFetchError> =>
  mastodonDeleteJson(`/api/v1/featured_tags/${encodeURIComponent(id)}`).map(() => undefined);
