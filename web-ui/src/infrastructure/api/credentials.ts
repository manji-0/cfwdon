import { type ResultAsync } from "neverthrow";
import type { AccountCredentials } from "@/domain/account/credentials";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import {
  mastodonFetchJson,
  mastodonPatchForm,
  mastodonPatchJson,
} from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import { parseAccountCredentials } from "@/infrastructure/mastodon/parsers/account";

export const fetchAccountCredentials = (): ResultAsync<AccountCredentials, MastodonFetchError> =>
  mastodonFetchJson("/api/v1/accounts/verify_credentials").andThen((raw) =>
    parseMastodon(parseAccountCredentials, raw),
  );

export type UpdateProfileInput = Readonly<{
  displayName?: string;
  note?: string;
  avatar?: File;
  header?: File;
}>;

const PROFILE_IMAGE_MAX_BYTES = 10 * 1024 * 1024;

export const profileImageAccept = "image/*";

export const profileImageTooLarge = (file: File): boolean => file.size > PROFILE_IMAGE_MAX_BYTES;

export const updateAccountProfile = (
  input: UpdateProfileInput,
): ResultAsync<AccountCredentials, MastodonFetchError> => {
  if (input.avatar || input.header) {
    const form = new FormData();
    if (input.displayName !== undefined) {
      form.append("display_name", input.displayName);
    }
    if (input.note !== undefined) {
      form.append("note", input.note);
    }
    if (input.avatar) {
      form.append("avatar", input.avatar);
    }
    if (input.header) {
      form.append("header", input.header);
    }
    return mastodonPatchForm("/api/v1/accounts/update_credentials", form).andThen((raw) =>
      parseMastodon(parseAccountCredentials, raw),
    );
  }

  const body: Record<string, unknown> = {};
  if (input.displayName !== undefined) {
    body.display_name = input.displayName;
  }
  if (input.note !== undefined) {
    body.note = input.note;
  }
  return mastodonPatchJson("/api/v1/accounts/update_credentials", body).andThen((raw) =>
    parseMastodon(parseAccountCredentials, raw),
  );
};
