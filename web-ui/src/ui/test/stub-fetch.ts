export type FetchHandler = (request: Request) => Response | Promise<Response>;

export type RecordedFetch = Readonly<{
  method: string;
  path: string;
  body: unknown;
}>;

const requestUrl = (input: RequestInfo | URL): string => {
  if (typeof input === "string") {
    return input;
  }
  if (input instanceof URL) {
    return input.href;
  }
  return input.url;
};

const pathnameOf = (raw: string): string => {
  const withoutOrigin =
    raw.startsWith("http://") || raw.startsWith("https://") ? new URL(raw).pathname : raw.split("?")[0];
  return withoutOrigin ?? raw;
};

const parseBody = (init?: RequestInit): unknown => {
  if (typeof init?.body !== "string" || init.body.length === 0) {
    return undefined;
  }
  try {
    return JSON.parse(init.body) as unknown;
  } catch {
    return init.body;
  }
};

export const jsonResponse = (body: unknown, status = 200): Response =>
  new Response(status === 204 ? null : JSON.stringify(body), {
    status,
    headers: status === 204 ? undefined : { "Content-Type": "application/json" },
  });

export const emptyResponse = (): Response => jsonResponse(null, 204);

export const stubFetch = (
  routes: Readonly<Record<string, unknown | FetchHandler>>,
): { recorded: RecordedFetch[]; restore: () => void } => {
  const recorded: RecordedFetch[] = [];
  const original = globalThis.fetch;
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const raw = requestUrl(input);
    const path = pathnameOf(raw);
    const method = (init?.method ?? (input instanceof Request ? input.method : "GET")).toUpperCase();
    recorded.push({ method, path, body: parseBody(init) });
    const handler = routes[`${method} ${path}`];
    if (handler === undefined) {
      throw new Error(`unexpected fetch ${method} ${path}`);
    }
    if (typeof handler === "function") {
      return handler(new Request(raw, init));
    }
    return jsonResponse(handler);
  }) as typeof fetch;
  return {
    recorded,
    restore: () => {
      globalThis.fetch = original;
    },
  };
};
