import { type ResultAsync } from "neverthrow";
import type { KeywordFilter } from "@/domain/filters/filter";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import {
  mastodonDeleteJson,
  mastodonFetchJson,
  mastodonPostJson,
} from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import {
  parseKeywordFilter,
  parseKeywordFilterList,
} from "@/infrastructure/mastodon/parsers/filter";

export type CreateKeywordFilterInput = Readonly<{
  title: string;
  context: ReadonlyArray<string>;
  keywords: ReadonlyArray<string>;
  filterAction: string;
}>;

export const fetchKeywordFilters = (): ResultAsync<
  ReadonlyArray<KeywordFilter>,
  MastodonFetchError
> =>
  mastodonFetchJson("/api/v2/filters").andThen((raw) =>
    parseMastodon(parseKeywordFilterList, raw),
  );

export const createKeywordFilter = (
  input: CreateKeywordFilterInput,
): ResultAsync<KeywordFilter, MastodonFetchError> =>
  mastodonPostJson("/api/v2/filters", {
    title: input.title,
    context: input.context,
    filter_action: input.filterAction,
    keywords: input.keywords
      .map((keyword) => keyword.trim())
      .filter((keyword) => keyword.length > 0)
      .map((keyword) => ({ keyword, whole_word: false })),
  }).andThen((raw) => parseMastodon(parseKeywordFilter, raw));

export const deleteKeywordFilter = (filterId: string): ResultAsync<void, MastodonFetchError> =>
  mastodonDeleteJson(`/api/v2/filters/${encodeURIComponent(filterId)}`).map(() => undefined);
