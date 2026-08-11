import { okAsync, type ResultAsync } from "neverthrow";
import { TrendTag } from "@/domain/trends/trend";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";

/** TODO(Phase 1): Fetch trending tags for the home sidebar. */
export const fetchTrendingTags = (): ResultAsync<ReadonlyArray<TrendTag>, MastodonFetchError> =>
  okAsync([]);
