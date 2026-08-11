import { errAsync, okAsync, type ResultAsync } from "neverthrow";
import { AccountCredentials } from "@/domain/account/credentials";
import { AccountPreferences } from "@/domain/settings/preferences";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonFetchJson, mastodonPatchJson } from "@/infrastructure/http/mastodon-fetch";

export const fetchAccountPreferences = (): ResultAsync<AccountPreferences, MastodonFetchError> =>
  mastodonFetchJson("/api/v1/preferences").andThen((raw) => {
    const parsed = AccountPreferences.schema.safeParse(raw);
    if (!parsed.success) {
      return errAsync({ kind: "ValidationError" } as const);
    }
    return okAsync(parsed.data);
  });

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
  return mastodonPatchJson("/api/v1/accounts/update_credentials", { source }).andThen((raw) => {
    const parsed = AccountCredentials.schema.safeParse(raw);
    if (!parsed.success) {
      return errAsync({ kind: "ValidationError" } as const);
    }
    return okAsync(parsed.data);
  });
};
