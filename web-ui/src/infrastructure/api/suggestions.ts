import { type ResultAsync } from "neverthrow";
import type { AccountSuggestion } from "@/domain/suggestions/suggestion";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonDeleteJson, mastodonFetchJson } from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import { parseAccountSuggestionList } from "@/infrastructure/mastodon/parsers/suggestion";

export const fetchSuggestions = (): ResultAsync<
  ReadonlyArray<AccountSuggestion>,
  MastodonFetchError
> =>
  mastodonFetchJson("/api/v2/suggestions").andThen((raw) =>
    parseMastodon(parseAccountSuggestionList, raw),
  );

export const dismissSuggestion = (accountId: string): ResultAsync<void, MastodonFetchError> =>
  mastodonDeleteJson(`/api/v1/suggestions/${encodeURIComponent(accountId)}`).map(() => undefined);
