import { type ResultAsync } from "neverthrow";
import type { AccountProfile } from "@/domain/account/account";
import type { Status } from "@/domain/status/status";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonFetchJson } from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import {
  parseAccountProfile,
  parseAccountProfileList,
} from "@/infrastructure/mastodon/parsers/account";
import { parseStatusList } from "@/infrastructure/mastodon/parsers/status";

export type AccountStatusesQuery = Readonly<{
  maxId?: string;
  limit?: number;
  excludeReplies?: boolean;
}>;

export type AccountCollectionQuery = Readonly<{
  maxId?: string;
  limit?: number;
}>;

export const fetchAccountProfile = (
  accountId: string,
): ResultAsync<AccountProfile, MastodonFetchError> =>
  mastodonFetchJson(`/api/v1/accounts/${encodeURIComponent(accountId)}`).andThen((raw) =>
    parseMastodon(parseAccountProfile, raw),
  );

export const fetchAccountStatuses = (
  accountId: string,
  query: AccountStatusesQuery = {},
): ResultAsync<ReadonlyArray<Status>, MastodonFetchError> => {
  const params = new URLSearchParams();
  params.set("limit", String(query.limit ?? 20));
  if (query.maxId) {
    params.set("max_id", query.maxId);
  }
  if (query.excludeReplies) {
    params.set("exclude_replies", "true");
  }
  return mastodonFetchJson(
    `/api/v1/accounts/${encodeURIComponent(accountId)}/statuses?${params}`,
  ).andThen((raw) => parseMastodon(parseStatusList, raw));
};

const fetchAccountCollection = (
  accountId: string,
  collection: "followers" | "following",
  query: AccountCollectionQuery = {},
): ResultAsync<ReadonlyArray<AccountProfile>, MastodonFetchError> => {
  const params = new URLSearchParams();
  params.set("limit", String(query.limit ?? 20));
  if (query.maxId) {
    params.set("max_id", query.maxId);
  }
  return mastodonFetchJson(
    `/api/v1/accounts/${encodeURIComponent(accountId)}/${collection}?${params}`,
  ).andThen((raw) => parseMastodon(parseAccountProfileList, raw));
};

export const fetchAccountFollowers = (
  accountId: string,
  query: AccountCollectionQuery = {},
): ResultAsync<ReadonlyArray<AccountProfile>, MastodonFetchError> =>
  fetchAccountCollection(accountId, "followers", query);

export const fetchAccountFollowing = (
  accountId: string,
  query: AccountCollectionQuery = {},
): ResultAsync<ReadonlyArray<AccountProfile>, MastodonFetchError> =>
  fetchAccountCollection(accountId, "following", query);
