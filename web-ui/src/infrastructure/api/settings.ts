import { type ResultAsync } from "neverthrow";
import { AccountPreferences } from "@/domain/settings/preferences";
import { notImplemented, type MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";

/** TODO(Phase 3): Load `/api/v1/preferences` and credential metadata. */
export const fetchAccountPreferences = (): ResultAsync<AccountPreferences, MastodonFetchError> =>
  notImplemented("account preferences");

/** TODO(Phase 3): PATCH preference fields and notification policy endpoints. */
export const updateAccountPreferences = (
  _input: Partial<AccountPreferences>,
): ResultAsync<AccountPreferences, MastodonFetchError> =>
  notImplemented("account preferences");
