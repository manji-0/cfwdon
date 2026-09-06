import { describe, expect, it } from "vitest";
import { createPagePrefetch } from "@/ui/lib/page-prefetch";
import { TIMELINE_PAGE_LIMIT } from "@/ui/lib/pagination";

const item = (id: string) => ({ id });
const page = (start: number, count: number) =>
  Array.from({ length: count }, (_, index) => item(`s${start + index}`));

describe("createPagePrefetch", () => {
  it("reveals a buffered page without fetching again, then prefetches the next", async () => {
    const fetched: string[] = [];
    const prefetch = createPagePrefetch(async (maxId) => {
      fetched.push(maxId);
      if (maxId === "s19") {
        return page(20, TIMELINE_PAGE_LIMIT);
      }
      if (maxId === "s39") {
        return page(40, 5);
      }
      return [];
    });

    const first = page(0, TIMELINE_PAGE_LIMIT);
    prefetch.prepareNext(first, first.length);
    await Promise.resolve();
    expect(fetched).toEqual(["s19"]);
    expect(prefetch.isReady()).toBe(true);

    const second = await prefetch.takeNext("s19");
    expect(second).toEqual(page(20, TIMELINE_PAGE_LIMIT));
    expect(fetched).toEqual(["s19"]);

    const shown = [...first, ...second];
    prefetch.prepareNext(shown, second.length);
    const third = await prefetch.takeNext("s39");
    expect(third).toEqual(page(40, 5));
    expect(fetched).toEqual(["s19", "s39"]);
  });

  it("does not prefetch when the shown page is short", async () => {
    const fetched: string[] = [];
    const prefetch = createPagePrefetch(async (maxId) => {
      fetched.push(maxId);
      return page(20, TIMELINE_PAGE_LIMIT);
    });

    prefetch.prepareNext(page(0, 5), 5);
    expect(prefetch.isReady()).toBe(true);
    expect(await prefetch.takeNext("s4")).toEqual([]);
    expect(fetched).toEqual([]);
  });

  it("cancels an in-flight prefetch when prepareNext runs again", async () => {
    const fetched: string[] = [];
    let releaseFirst!: (value: ReadonlyArray<{ id: string }>) => void;
    const firstFetch = new Promise<ReadonlyArray<{ id: string }>>((resolve) => {
      releaseFirst = resolve;
    });
    let calls = 0;
    const prefetch = createPagePrefetch(async (maxId) => {
      fetched.push(maxId);
      calls += 1;
      if (calls === 1) {
        return firstFetch;
      }
      return page(100, TIMELINE_PAGE_LIMIT);
    });

    prefetch.prepareNext(page(0, TIMELINE_PAGE_LIMIT), TIMELINE_PAGE_LIMIT);
    prefetch.prepareNext(page(50, TIMELINE_PAGE_LIMIT), TIMELINE_PAGE_LIMIT);
    releaseFirst(page(20, TIMELINE_PAGE_LIMIT));

    const next = await prefetch.takeNext("s69");
    expect(next).toEqual(page(100, TIMELINE_PAGE_LIMIT));
    expect(fetched).toEqual(["s19", "s69"]);
  });

  it("falls back to a live fetch when nothing is prepared", async () => {
    const prefetch = createPagePrefetch(async (maxId) => {
      expect(maxId).toBe("s19");
      return page(20, 3);
    });
    await expect(prefetch.takeNext("s19")).resolves.toEqual(page(20, 3));
  });
});
