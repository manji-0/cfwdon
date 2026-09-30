export type HttpError = Readonly<
  | { kind: "HttpStatus"; status: number; body: string }
  | { kind: "NetworkError"; cause: unknown }
>;

export type ValidationError = Readonly<{ kind: "ValidationError" }>;

/** Every failure a Mastodon API call can surface to the application layer. */
export type MastodonFetchError = HttpError | ValidationError;

export const HttpError = {
  fromResponse: async (response: Response): Promise<HttpError> => ({
    kind: "HttpStatus",
    status: response.status,
    body: await response.text(),
  }),

  fromUnknown: (cause: unknown): HttpError => ({
    kind: "NetworkError",
    cause,
  }),
} as const;
