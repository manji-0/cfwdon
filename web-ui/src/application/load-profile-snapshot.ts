import { ResultAsync } from "neverthrow";
import type { ProfileSnapshot } from "@/domain/cache/profile-set";
import type { MastodonFetchError } from "@/domain/errors/http-error";

/** Port implemented by `infrastructure/api/account`. */
export type ProfileSnapshotSource = Readonly<{
  fetchAccountProfile: (
    accountId: string,
  ) => ResultAsync<ProfileSnapshot["profile"], MastodonFetchError>;
  fetchAccountStatuses: (
    accountId: string,
    query: Readonly<{ excludeReplies: boolean }>,
  ) => ResultAsync<ProfileSnapshot["statuses"], MastodonFetchError>;
}>;

export const loadProfileSnapshot = (
  source: ProfileSnapshotSource,
  accountId: string,
  now = Date.now(),
): ResultAsync<ProfileSnapshot, MastodonFetchError> =>
  ResultAsync.combine([
    source.fetchAccountProfile(accountId),
    source.fetchAccountStatuses(accountId, { excludeReplies: true }),
  ]).map(([profile, statuses]) => ({
    profile,
    statuses,
    fetchedAt: now,
    scrollY: 0,
  }));
