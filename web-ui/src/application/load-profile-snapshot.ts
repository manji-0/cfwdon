import { ResultAsync } from "neverthrow";
import type { ProfileSnapshot } from "@/domain/cache/profile-set";
import { fetchAccountProfile, fetchAccountStatuses } from "@/infrastructure/api/account";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";

export const loadProfileSnapshot = (
  accountId: string,
  now = Date.now(),
): ResultAsync<ProfileSnapshot, MastodonFetchError> =>
  ResultAsync.combine([
    fetchAccountProfile(accountId),
    fetchAccountStatuses(accountId, { excludeReplies: true }),
  ]).map(([profile, statuses]) => ({
    profile,
    statuses,
    fetchedAt: now,
    scrollY: 0,
  }));
