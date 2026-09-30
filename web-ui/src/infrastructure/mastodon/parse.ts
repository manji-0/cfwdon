import { type } from "arktype";
import { errAsync, okAsync, type ResultAsync } from "neverthrow";
import type { MastodonFetchError } from "@/domain/errors/http-error";

export type ArkParser<T> = (input: unknown) => T | type.errors;

export const parseMastodon = <T>(
  parser: ArkParser<T>,
  raw: unknown,
): ResultAsync<T, MastodonFetchError> => {
  const result = parser(raw);
  if (result instanceof type.errors) {
    return errAsync({ kind: "ValidationError" } as const);
  }
  return okAsync(result);
};

export const isArkError = (value: unknown): value is type.errors => value instanceof type.errors;
