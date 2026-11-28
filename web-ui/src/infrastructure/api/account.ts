import { type ResultAsync } from "neverthrow";
import type { AccountProfile } from "@/domain/account/account";
import type { Status } from "@/domain/status/status";
import type { MastodonFetchError } from "@/domain/errors/http-error";
import type { Page, PageQuery } from "@/domain/pagination";
import { mastodonFetchJson, mastodonFetchPage, pageParams } from "@/infrastructure/http/mastodon-fetch";
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
  onlyMedia?: boolean;
  pinned?: boolean;
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
  const params = pageParams(query);
  if (query.excludeReplies) {
    params.set("exclude_replies", "true");
  }
  if (query.onlyMedia) {
    params.set("only_media", "true");
  }
  if (query.pinned) {
    params.set("pinned", "true");
  }
  return mastodonFetchJson(
    `/api/v1/accounts/${encodeURIComponent(accountId)}/statuses?${params}`,
  ).andThen((raw) => parseMastodon(parseStatusList, raw));
};

const fetchAccountCollection = (
  accountId: string,
  collection: "followers" | "following",
  query: PageQuery = {},
): ResultAsync<Page<AccountProfile>, MastodonFetchError> => {
  const params = pageParams(query);
  return mastodonFetchPage(
    `/api/v1/accounts/${encodeURIComponent(accountId)}/${collection}?${params}`,
    parseAccountProfileList,
  );
};

export const fetchAccountFollowers = (
  accountId: string,
  query: PageQuery = {},
): ResultAsync<Page<AccountProfile>, MastodonFetchError> =>
  fetchAccountCollection(accountId, "followers", query);

export const fetchAccountFollowing = (
  accountId: string,
  query: PageQuery = {},
): ResultAsync<Page<AccountProfile>, MastodonFetchError> =>
  fetchAccountCollection(accountId, "following", query);

/** Adapter for `application/load-profile-snapshot`'s `ProfileSnapshotSource` port. */
export const profileSnapshotSource = { fetchAccountProfile, fetchAccountStatuses } as const;
