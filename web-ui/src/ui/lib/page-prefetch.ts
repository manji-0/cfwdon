import { TIMELINE_PAGE_LIMIT, pageHasMore } from "@/ui/lib/pagination";

export type PrefetchItem = Readonly<{ id: string }>;

/**
 * A fetched page. `nextMaxId` is the server-provided cursor (null = last page);
 * undefined means the endpoint gives none and the last item's id is the cursor.
 */
export type FetchedPage<T> = Readonly<{
  items: ReadonlyArray<T>;
  nextMaxId?: string | null;
}>;

/** Wrap a bare array (cursor = last item id) as a FetchedPage. */
export const toFetchedPage = <T>(fetched: ReadonlyArray<T> | FetchedPage<T>): FetchedPage<T> =>
  Array.isArray(fetched) ? { items: fetched as ReadonlyArray<T> } : (fetched as FetchedPage<T>);

/** True when more pages may follow: the server cursor if given, else a full page. */
export const fetchedPageHasMore = <T>(page: FetchedPage<T>): boolean =>
  page.nextMaxId === undefined ? pageHasMore(page.items.length) : page.nextMaxId !== null;

const EXHAUSTED_PAGE ={ items: [], nextMaxId: null } as const;

type PrefetchResult<T> =
  | { kind: "items"; value: FetchedPage<T> }
  | { kind: "exhausted" }
  | { kind: "cancelled" };

export type PagePrefetch<T extends PrefetchItem> = Readonly<{
  reset: () => void;
  /** `nextMaxId`, when the endpoint supplies one, replaces the last item's id as cursor. */
  prepareNext: (
    shownItems: ReadonlyArray<T>,
    fetchedCount: number,
    nextMaxId?: string | null,
  ) => void;
  takeNext: (fallbackMaxId: string) => Promise<ReadonlyArray<T>>;
  takeNextPage: (fallbackMaxId: string) => Promise<FetchedPage<T>>;
  isReady: () => boolean;
}>;

const consume = <T>(result: PrefetchResult<T>): FetchedPage<T> | null => {
  switch (result.kind) {
    case "exhausted":
      return EXHAUSTED_PAGE;
    case "items":
      return result.value;
    case "cancelled":
      return null;
  }
};

/** One-page lookahead: fetch the next page in the background, then reveal it on demand. */
export const createPagePrefetch = <T extends PrefetchItem>(
  fetchNext: (maxId: string) => Promise<ReadonlyArray<T> | FetchedPage<T>>,
  limit = TIMELINE_PAGE_LIMIT,
): PagePrefetch<T> => {
  let generation = 0;
  let buffer: PrefetchResult<T> | null = null;
  let inflight: Promise<PrefetchResult<T>> | null = null;

  const fetchPage = async (maxId: string): Promise<FetchedPage<T>> =>
    toFetchedPage(await fetchNext(maxId));

  const reset = () => {
    generation += 1;
    buffer = null;
    inflight = null;
  };

  const prepareNext = (
    shownItems: ReadonlyArray<T>,
    fetchedCount: number,
    nextMaxId?: string | null,
  ) => {
    generation += 1;
    buffer = null;
    const currentGeneration = generation;
    const cursor = nextMaxId === undefined ? shownItems.at(-1)?.id : nextMaxId;
    const more = nextMaxId === undefined ? pageHasMore(fetchedCount, limit) : true;
    if (!cursor || !more) {
      inflight = Promise.resolve({ kind: "exhausted" });
      buffer = { kind: "exhausted" };
      return;
    }
    inflight = fetchNext(cursor)
      .then((fetched): PrefetchResult<T> => {
        if (currentGeneration !== generation) {
          return { kind: "cancelled" };
        }
        const value = toFetchedPage(fetched);
        const result: PrefetchResult<T> =
          value.items.length === 0 ? { kind: "exhausted" } : { kind: "items", value };
        buffer = result;
        return result;
      })
      .catch((error: unknown) => {
        if (currentGeneration !== generation) {
          return { kind: "cancelled" as const };
        }
        inflight = null;
        buffer = null;
        throw error;
      });
    void inflight.catch(() => undefined);
  };

  const takeNextPage = async (fallbackMaxId: string): Promise<FetchedPage<T>> => {
    if (buffer) {
      const result = buffer;
      buffer = null;
      inflight = null;
      return consume(result) ?? EXHAUSTED_PAGE;
    }
    if (inflight) {
      try {
        const result = await inflight;
        buffer = null;
        inflight = null;
        const consumed = consume(result);
        if (consumed !== null) {
          return consumed;
        }
      } catch (error) {
        inflight = null;
        buffer = null;
        throw error;
      }
    }
    return fetchPage(fallbackMaxId);
  };

  const takeNext = async (fallbackMaxId: string): Promise<ReadonlyArray<T>> =>
    (await takeNextPage(fallbackMaxId)).items;

  const isReady = () => buffer !== null;

  return { reset, prepareNext, takeNext, takeNextPage, isReady };
};
