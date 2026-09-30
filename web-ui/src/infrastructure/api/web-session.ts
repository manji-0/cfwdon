import { errAsync, okAsync, type ResultAsync } from "neverthrow";
import type { MastodonFetchError } from "@/domain/errors/http-error";
import type { AccountSummary } from "@/domain/session/account";
import { mastodonFetchJson } from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import { parseAccountSummary } from "@/infrastructure/mastodon/parsers/account";

const SESSION_PATH = "/api/cfwdon/web/session";

/** Resolve the signed-in account; `null` when the worker answers 401 (signed out). */
export const fetchWebSession = (): ResultAsync<AccountSummary | null, MastodonFetchError> =>
  mastodonFetchJson(SESSION_PATH)
    .andThen((raw) => parseMastodon(parseAccountSummary, raw))
    .orElse((error) =>
      error.kind === "HttpStatus" && error.status === 401
        ? okAsync<AccountSummary | null, MastodonFetchError>(null)
        : errAsync<AccountSummary | null, MastodonFetchError>(error),
    );
