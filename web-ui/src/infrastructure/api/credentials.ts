import { errAsync, okAsync, type ResultAsync } from "neverthrow";
import { AccountCredentials } from "@/domain/account/credentials";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonFetchJson, mastodonPatchJson } from "@/infrastructure/http/mastodon-fetch";

export const fetchAccountCredentials = (): ResultAsync<AccountCredentials, MastodonFetchError> =>
  mastodonFetchJson("/api/v1/accounts/verify_credentials").andThen((raw) => {
    const parsed = AccountCredentials.schema.safeParse(raw);
    if (!parsed.success) {
      return errAsync({ kind: "ValidationError" } as const);
    }
    return okAsync(parsed.data);
  });

export type UpdateProfileInput = Readonly<{
  displayName?: string;
  note?: string;
}>;

export const updateAccountProfile = (
  input: UpdateProfileInput,
): ResultAsync<AccountCredentials, MastodonFetchError> => {
  const body: Record<string, unknown> = {};
  if (input.displayName !== undefined) {
    body.display_name = input.displayName;
  }
  if (input.note !== undefined) {
    body.note = input.note;
  }
  return mastodonPatchJson("/api/v1/accounts/update_credentials", body).andThen((raw) => {
    const parsed = AccountCredentials.schema.safeParse(raw);
    if (!parsed.success) {
      return errAsync({ kind: "ValidationError" } as const);
    }
    return okAsync(parsed.data);
  });
};
