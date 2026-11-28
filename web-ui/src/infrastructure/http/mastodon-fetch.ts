import { errAsync, okAsync, ResultAsync } from "neverthrow";
import { HttpError, type MastodonFetchError } from "@/domain/errors/http-error";
import type { Page, PageQuery } from "@/domain/pagination";
import { parseMastodon, type ArkParser } from "@/infrastructure/mastodon/parse";

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

/**
 * `max_id` of the `rel="next"` entry in a `Link` response header, or null when
 * there is no next page. Some endpoints page by an internal numeric cursor that
 * differs from the item ids, so the header is the only reliable source.
 */
export const nextMaxIdFromLinkHeader = (header: string | null): string | null => {
  if (!header) {
    return null;
  }
  for (const entry of header.matchAll(/<([^>]*)>((?:\s*;\s*[^;,<]+)*)/g)) {
    const [, target = "", params = ""] = entry;
    const rel = /;\s*rel\s*=\s*"?([^";,]*)"?/i.exec(params)?.[1] ?? "";
    if (!rel.toLowerCase().split(/\s+/).includes("next")) {
      continue;
    }
    try {
      return new URL(target, "http://localhost").searchParams.get("max_id") || null;
    } catch {
      return null;
    }
  }
  return null;
};

const fetchOkResponse = (
  path: string,
  init: RequestInit = {},
): ResultAsync<Response, MastodonFetchError> =>
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
  ).andThen((response): ResultAsync<Response, MastodonFetchError> => {
    if (!response.ok) {
      return ResultAsync.fromPromise(
        HttpError.fromResponse(response),
        HttpError.fromUnknown,
      ).andThen((error) => errAsync(error));
    }
    return okAsync(response);
  });

export const mastodonFetchJson = (
  path: string,
  init: RequestInit = {},
): JsonResult<unknown> =>
  fetchOkResponse(path, init).andThen((response): JsonResult<unknown> =>
    response.status === 204 ? okAsync(null) : parseJson(response),
  );

/** GET a paginated collection and page it by the `Link: rel="next"` cursor. */
export const mastodonFetchPage = <T>(
  path: string,
  parser: ArkParser<ReadonlyArray<T>>,
): ResultAsync<Page<T>, MastodonFetchError> =>
  fetchOkResponse(path).andThen((response) =>
    parseJson(response).andThen((raw) =>
      parseMastodon(parser, raw).map(
        (items): Page<T> => ({
          items,
          nextMaxId: nextMaxIdFromLinkHeader(response.headers.get("Link")),
        }),
      ),
    ),
  );

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
