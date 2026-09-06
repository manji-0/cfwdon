import { type ResultAsync } from "neverthrow";
import type { KeywordFilter } from "@/domain/filters/filter";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import {
  mastodonDeleteJson,
  mastodonFetchJson,
  mastodonPostJson,
  mastodonPutJson,
} from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import {
  parseKeywordFilter,
  parseKeywordFilterList,
} from "@/infrastructure/mastodon/parsers/filter";

export type KeywordFilterInput = Readonly<{
  title: string;
  context: ReadonlyArray<string>;
  keywords: ReadonlyArray<string>;
  filterAction: string;
  expiresIn?: number;
}>;

const filterBody = (input: KeywordFilterInput): Record<string, unknown> => ({
  title: input.title,
  context: [...input.context],
  filter_action: input.filterAction,
  ...(input.expiresIn !== undefined ? { expires_in: input.expiresIn } : {}),
  keywords: input.keywords
    .map((keyword) => keyword.trim())
    .filter((keyword) => keyword.length > 0)
    .map((keyword) => ({ keyword, whole_word: false })),
});

export const fetchKeywordFilters = (): ResultAsync<
  ReadonlyArray<KeywordFilter>,
  MastodonFetchError
> =>
  mastodonFetchJson("/api/v2/filters").andThen((raw) =>
    parseMastodon(parseKeywordFilterList, raw),
  );

export const createKeywordFilter = (
  input: KeywordFilterInput,
): ResultAsync<KeywordFilter, MastodonFetchError> =>
  mastodonPostJson("/api/v2/filters", filterBody(input)).andThen((raw) =>
    parseMastodon(parseKeywordFilter, raw),
  );

export const updateKeywordFilter = (
  filterId: string,
  input: KeywordFilterInput,
): ResultAsync<KeywordFilter, MastodonFetchError> =>
  mastodonPutJson(`/api/v2/filters/${encodeURIComponent(filterId)}`, filterBody(input)).andThen(
    (raw) => parseMastodon(parseKeywordFilter, raw),
  );

export const deleteKeywordFilter = (filterId: string): ResultAsync<void, MastodonFetchError> =>
  mastodonDeleteJson(`/api/v2/filters/${encodeURIComponent(filterId)}`).map(() => undefined);
