import { type } from "arktype";
import { errAsync, okAsync, ResultAsync } from "neverthrow";
import { HttpError } from "@/domain/errors/http-error";
import type { AccountSummary } from "@/domain/session/account";
import { parseAccountSummary } from "@/infrastructure/mastodon/parsers/account";

const SESSION_PATH = "/api/cfwdon/web/session";

export type FetchSessionError =
  | HttpError
  | Readonly<{ kind: "ValidationError" }>;

export const fetchWebSession = (): ResultAsync<AccountSummary | null, FetchSessionError> =>
  ResultAsync.fromPromise(
    fetch(SESSION_PATH, {
      credentials: "same-origin",
      headers: { Accept: "application/json" },
    }),
    HttpError.fromUnknown,
  ).andThen((response): ResultAsync<AccountSummary | null, FetchSessionError> => {
    if (response.status === 401) {
      return okAsync(null);
    }
    if (!response.ok) {
      return ResultAsync.fromPromise(
        HttpError.fromResponse(response),
        HttpError.fromUnknown,
      ).andThen((error) => errAsync(error));
    }
    return ResultAsync.fromPromise(response.json(), HttpError.fromUnknown).andThen((raw) => {
      const result = parseAccountSummary(raw);
      if (result instanceof type.errors) {
        return errAsync({ kind: "ValidationError" } as const);
      }
      return okAsync(result);
    });
  });
