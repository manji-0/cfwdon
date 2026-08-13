import { errAsync, okAsync, ResultAsync } from "neverthrow";
import { HttpError } from "@/domain/errors/http-error";

export type ValidationError = Readonly<{ kind: "ValidationError" }>;

export type NotImplementedError = Readonly<{ kind: "NotImplemented"; feature: string }>;

export type MastodonFetchError = HttpError | ValidationError | NotImplementedError;

export const notImplemented = (feature: string): ResultAsync<never, NotImplementedError> =>
  errAsync({ kind: "NotImplemented", feature } as const);

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

export const mastodonUploadFile = (
  path: string,
  file: File,
): JsonResult<unknown> => {
  const form = new FormData();
  form.append("file", file);
  return ResultAsync.fromPromise(
    fetch(path, {
      method: "POST",
      credentials: "same-origin",
      headers: { Accept: "application/json" },
      body: form,
    }),
    HttpError.fromUnknown,
  ).andThen((response): JsonResult<unknown> => {
    if (!response.ok) {
      return ResultAsync.fromPromise(
        HttpError.fromResponse(response),
        HttpError.fromUnknown,
      ).andThen((error) => errAsync(error));
    }
    return parseJson(response);
  });
};

export const mastodonPostJson = (
  path: string,
  body: unknown,
): JsonResult<unknown> =>
  mastodonFetchJson(path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });

export const mastodonPatchJson = (
  path: string,
  body: unknown,
): JsonResult<unknown> =>
  mastodonFetchJson(path, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });

export const mastodonPatchForm = (path: string, form: FormData): JsonResult<unknown> =>
  mastodonFetchJson(path, {
    method: "PATCH",
    body: form,
  });

export const mastodonPutJson = (
  path: string,
  body: unknown,
): JsonResult<unknown> =>
  mastodonFetchJson(path, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });

export const mastodonDeleteJson = (
  path: string,
  body?: unknown,
): JsonResult<unknown> =>
  mastodonFetchJson(path, {
    method: "DELETE",
    headers: body === undefined ? undefined : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
