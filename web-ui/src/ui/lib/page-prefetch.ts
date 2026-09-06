import { TIMELINE_PAGE_LIMIT, pageHasMore } from "@/ui/lib/pagination";

export type PrefetchItem = Readonly<{ id: string }>;

type PrefetchResult<T> =
  | { kind: "items"; value: ReadonlyArray<T> }
  | { kind: "exhausted" }
  | { kind: "cancelled" };

export type PagePrefetch<T extends PrefetchItem> = Readonly<{
  reset: () => void;
  prepareNext: (shownItems: ReadonlyArray<T>, fetchedCount: number) => void;
  takeNext: (fallbackMaxId: string) => Promise<ReadonlyArray<T>>;
  isReady: () => boolean;
}>;

const consume = <T>(result: PrefetchResult<T>): ReadonlyArray<T> | null => {
  switch (result.kind) {
    case "exhausted":
      return [];
    case "items":
      return result.value;
    case "cancelled":
      return null;
  }
};

/** One-page lookahead: fetch the next page in the background, then reveal it on demand. */
export const createPagePrefetch = <T extends PrefetchItem>(
  fetchNext: (maxId: string) => Promise<ReadonlyArray<T>>,
  limit = TIMELINE_PAGE_LIMIT,
): PagePrefetch<T> => {
  let generation = 0;
  let buffer: PrefetchResult<T> | null = null;
  let inflight: Promise<PrefetchResult<T>> | null = null;

  const reset = () => {
    generation += 1;
    buffer = null;
    inflight = null;
  };

  const prepareNext = (shownItems: ReadonlyArray<T>, fetchedCount: number) => {
    generation += 1;
    buffer = null;
    const currentGeneration = generation;
    const last = shownItems.at(-1);
    if (!last || !pageHasMore(fetchedCount, limit)) {
      inflight = Promise.resolve({ kind: "exhausted" });
      buffer = { kind: "exhausted" };
      return;
    }
    inflight = fetchNext(last.id)
      .then((value): PrefetchResult<T> => {
        if (currentGeneration !== generation) {
          return { kind: "cancelled" };
        }
        const result: PrefetchResult<T> =
          value.length === 0 ? { kind: "exhausted" } : { kind: "items", value };
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

  const takeNext = async (fallbackMaxId: string): Promise<ReadonlyArray<T>> => {
    if (buffer) {
      const result = buffer;
      buffer = null;
      inflight = null;
      return consume(result) ?? [];
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
    return fetchNext(fallbackMaxId);
  };

  const isReady = () => buffer !== null;

  return { reset, prepareNext, takeNext, isReady };
};
