import { okAsync, type ResultAsync } from "neverthrow";
import type { Relationship } from "@/domain/account/relationship";
import { Relationship as RelationshipModel } from "@/domain/account/relationship";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonFetchJson, mastodonPostJson } from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import {
  parseRelationship,
  parseRelationshipList,
} from "@/infrastructure/mastodon/parsers/relationship";

const mapRelationship = (raw: unknown): ResultAsync<Relationship, MastodonFetchError> =>
  parseMastodon(parseRelationship, raw);

export const fetchRelationship = (
  accountId: string,
): ResultAsync<Relationship, MastodonFetchError> => {
  const params = new URLSearchParams();
  params.append("id[]", accountId);
  return mastodonFetchJson(`/api/v1/accounts/relationships?${params}`).andThen((raw) =>
    parseMastodon(parseRelationshipList, raw).andThen((list) =>
      okAsync(list[0] ?? RelationshipModel.empty(accountId)),
    ),
  );
};

export const followAccount = (accountId: string): ResultAsync<Relationship, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/accounts/${encodeURIComponent(accountId)}/follow`, {}).andThen(
    mapRelationship,
  );

export const unfollowAccount = (accountId: string): ResultAsync<Relationship, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/accounts/${encodeURIComponent(accountId)}/unfollow`, {}).andThen(
    mapRelationship,
  );

export const muteAccount = (accountId: string): ResultAsync<Relationship, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/accounts/${encodeURIComponent(accountId)}/mute`, {}).andThen(
    mapRelationship,
  );

export const unmuteAccount = (accountId: string): ResultAsync<Relationship, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/accounts/${encodeURIComponent(accountId)}/unmute`, {}).andThen(
    mapRelationship,
  );

export const blockAccount = (accountId: string): ResultAsync<Relationship, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/accounts/${encodeURIComponent(accountId)}/block`, {}).andThen(
    mapRelationship,
  );

export const unblockAccount = (accountId: string): ResultAsync<Relationship, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/accounts/${encodeURIComponent(accountId)}/unblock`, {}).andThen(
    mapRelationship,
  );

export const authorizeFollowRequest = (
  accountId: string,
): ResultAsync<Relationship, MastodonFetchError> =>
  mastodonPostJson(
    `/api/v1/follow_requests/${encodeURIComponent(accountId)}/authorize`,
    {},
  ).andThen(mapRelationship);

export const rejectFollowRequest = (
  accountId: string,
): ResultAsync<Relationship, MastodonFetchError> =>
  mastodonPostJson(
    `/api/v1/follow_requests/${encodeURIComponent(accountId)}/reject`,
    {},
  ).andThen(mapRelationship);
