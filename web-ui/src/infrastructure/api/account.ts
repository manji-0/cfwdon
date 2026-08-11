import { errAsync, okAsync, ResultAsync } from "neverthrow";
import type { AccountProfile } from "@/domain/account/account";
import { AccountProfile as AccountProfileModel } from "@/domain/account/account";
import type { Status } from "@/domain/status/status";
import { StatusListSchema } from "@/domain/status/status";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonFetchJson } from "@/infrastructure/http/mastodon-fetch";

export const fetchAccountProfile = (
  accountId: string,
): ResultAsync<AccountProfile, MastodonFetchError> =>
  mastodonFetchJson(`/api/v1/accounts/${encodeURIComponent(accountId)}`).andThen((raw) => {
    const parsed = AccountProfileModel.schema.safeParse(raw);
    if (!parsed.success) {
      return errAsync({ kind: "ValidationError" } as const);
    }
    return okAsync(parsed.data);
  });

export type AccountStatusesQuery = Readonly<{
  maxId?: string;
  limit?: number;
  excludeReplies?: boolean;
}>;

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
  ).andThen((raw) => {
    const parsed = StatusListSchema.safeParse(raw);
    if (!parsed.success) {
      return errAsync({ kind: "ValidationError" } as const);
    }
    return okAsync(parsed.data);
  });
};
