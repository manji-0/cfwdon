import { type ResultAsync } from "neverthrow";
import type { AccountPreferences } from "@/domain/settings/preferences";
import type { AccountCredentials } from "@/domain/account/credentials";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonFetchJson, mastodonPatchJson } from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import { parseAccountCredentials } from "@/infrastructure/mastodon/parsers/account";
import { parseAccountPreferences } from "@/infrastructure/mastodon/parsers/preferences";

export const fetchAccountPreferences = (): ResultAsync<AccountPreferences, MastodonFetchError> =>
  mastodonFetchJson("/api/v1/preferences").andThen((raw) =>
    parseMastodon(parseAccountPreferences, raw),
  );

export type UpdatePostingPreferencesInput = Readonly<{
  defaultVisibility?: string;
  defaultSensitive?: boolean;
  defaultLanguage?: string | null;
  defaultQuotePolicy?: string;
}>;

/** Posting defaults are updated via `update_credentials` source fields. */
export const updatePostingPreferences = (
  input: UpdatePostingPreferencesInput,
): ResultAsync<AccountCredentials, MastodonFetchError> => {
  const source: Record<string, unknown> = {};
  if (input.defaultVisibility !== undefined) {
    source.privacy = input.defaultVisibility;
  }
  if (input.defaultSensitive !== undefined) {
    source.sensitive = input.defaultSensitive;
  }
  if (input.defaultLanguage !== undefined) {
    source.language = input.defaultLanguage;
  }
  if (input.defaultQuotePolicy !== undefined) {
    source.quote_policy = input.defaultQuotePolicy;
  }
  return mastodonPatchJson("/api/v1/accounts/update_credentials", { source }).andThen((raw) =>
    parseMastodon(parseAccountCredentials, raw),
  );
};
