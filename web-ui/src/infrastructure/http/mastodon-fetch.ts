import { errAsync, okAsync, ResultAsync } from "neverthrow";
import { HttpError, type MastodonFetchError } from "@/domain/errors/http-error";
import type { PageQuery } from "@/domain/pagination";

type JsonResult<T> = ResultAsync<T, MastodonFetchError>;

/** `limit` (default 20) and optional `max_id` params shared by paginated endpoints. */
export const pageParams = (query: PageQuery): URLSearchParams => {
  const params = new URLSearchParams();
  params.set("limit", String(query.limit ?? 20));
  if (query.maxId) {
    params.set("max_id", query.maxId);
  }
  return params;
};

const parseJson = (response: Response): JsonResult<unknown> =>
  ResultAsync.fromPromise(response.json(), HttpError.fromUnknown);

export const mastodonFetchJson = (
  path: string,
  init: RequestInit = {},
): JsonResult<unknown> =>
  ResultAsync.fromPromise(
    fetch(path, {
      ...init,
      credentials: "same-origin",
      headers: {
        Accept: "application/json",
        ...(init.headers ?? {}),
      },
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

const sendJson =
  (method: "POST" | "PATCH" | "PUT" | "DELETE") =>
  (path: string, body?: unknown): JsonResult<unknown> =>
    mastodonFetchJson(path, {
      method,
      headers: body === undefined ? undefined : { "Content-Type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });

export const mastodonPostJson: (path: string, body: unknown) => JsonResult<unknown> =
  sendJson("POST");
export const mastodonPatchJson: (path: string, body: unknown) => JsonResult<unknown> =
  sendJson("PATCH");
export const mastodonPutJson: (path: string, body: unknown) => JsonResult<unknown> =
  sendJson("PUT");
export const mastodonDeleteJson = sendJson("DELETE");

export const mastodonPatchForm = (path: string, form: FormData): JsonResult<unknown> =>
  mastodonFetchJson(path, { method: "PATCH", body: form });

export const mastodonUploadFile = (path: string, file: File): JsonResult<unknown> => {
  const form = new FormData();
  form.append("file", file);
  return mastodonFetchJson(path, { method: "POST", body: form });
};
