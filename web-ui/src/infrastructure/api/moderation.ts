import { type ResultAsync } from "neverthrow";
import type { AccountRef } from "@/domain/account/account";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonFetchJson } from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import { parseAccountList } from "@/infrastructure/mastodon/parsers/moderation";

export type ModerationListQuery = Readonly<{
  maxId?: string;
  limit?: number;
}>;

const buildQuery = (query: ModerationListQuery): string => {
  const params = new URLSearchParams();
  params.set("limit", String(query.limit ?? 20));
  if (query.maxId) {
    params.set("max_id", query.maxId);
  }
  return params.toString();
};

export const fetchMutedAccounts = (
  query: ModerationListQuery = {},
): ResultAsync<ReadonlyArray<AccountRef>, MastodonFetchError> =>
  mastodonFetchJson(`/api/v1/mutes?${buildQuery(query)}`).andThen((raw) =>
    parseMastodon(parseAccountList, raw),
  );

export const fetchBlockedAccounts = (
  query: ModerationListQuery = {},
): ResultAsync<ReadonlyArray<AccountRef>, MastodonFetchError> =>
  mastodonFetchJson(`/api/v1/blocks?${buildQuery(query)}`).andThen((raw) =>
    parseMastodon(parseAccountList, raw),
  );
