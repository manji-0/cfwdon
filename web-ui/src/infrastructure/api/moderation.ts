import { errAsync, okAsync, type ResultAsync } from "neverthrow";
import { AccountRef } from "@/domain/account/account";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonFetchJson } from "@/infrastructure/http/mastodon-fetch";

const parseAccountList = (raw: unknown): ResultAsync<ReadonlyArray<AccountRef>, MastodonFetchError> => {
  if (!Array.isArray(raw)) {
    return errAsync({ kind: "ValidationError" } as const);
  }
  const accounts: AccountRef[] = [];
  for (const item of raw) {
    const parsed = AccountRef.schema.safeParse(item);
    if (!parsed.success) {
      return errAsync({ kind: "ValidationError" } as const);
    }
    accounts.push(parsed.data);
  }
  return okAsync(accounts);
};

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
  mastodonFetchJson(`/api/v1/mutes?${buildQuery(query)}`).andThen(parseAccountList);

export const fetchBlockedAccounts = (
  query: ModerationListQuery = {},
): ResultAsync<ReadonlyArray<AccountRef>, MastodonFetchError> =>
  mastodonFetchJson(`/api/v1/blocks?${buildQuery(query)}`).andThen(parseAccountList);
