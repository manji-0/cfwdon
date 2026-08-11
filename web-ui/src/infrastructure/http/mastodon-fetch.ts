import { errAsync, okAsync, ResultAsync } from "neverthrow";
import { HttpError } from "@/domain/errors/http-error";

export type ValidationError = Readonly<{ kind: "ValidationError" }>;

export type MastodonFetchError = HttpError | ValidationError;

type JsonResult<T> = ResultAsync<T, MastodonFetchError>;

const parseJson = (response: Response): JsonResult<unknown> =>
  ResultAsync.fromPromise(response.json(), HttpError.fromUnknown);

export const mastodonFetchJson = (
  path: string,
  init: RequestInit = {},
): JsonResult<unknown> =>
  ResultAsync.fromPromise(
    fetch(path, {
      credentials: "same-origin",
      headers: {
        Accept: "application/json",
        ...(init.headers ?? {}),
      },
      ...init,
    }),
    HttpError.fromUnknown,
  ).andThen((response): JsonResult<unknown> => {
    if (!response.ok) {
      return ResultAsync.fromPromise(
        HttpError.fromResponse(response),
        HttpError.fromUnknown,
      ).andThen((error) => errAsync(error));
    }
    if (response.status === 204) {
      return okAsync(null);
    }
    return parseJson(response);
  });

export const mastodonPostJson = (
  path: string,
  body: unknown,
): JsonResult<unknown> =>
  mastodonFetchJson(path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
