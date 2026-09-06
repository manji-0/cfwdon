import { describe, expect, it } from "vitest";
import { TIMELINE_PAGE_LIMIT, pageHasMore } from "@/ui/lib/pagination";

describe("pageHasMore", () => {
  it("is true when the page is full", () => {
    expect(pageHasMore(TIMELINE_PAGE_LIMIT)).toBe(true);
    expect(pageHasMore(TIMELINE_PAGE_LIMIT + 1)).toBe(true);
  });

  it("is false when the page is short or empty", () => {
    expect(pageHasMore(0)).toBe(false);
    expect(pageHasMore(TIMELINE_PAGE_LIMIT - 1)).toBe(false);
  });

  it("uses the provided limit", () => {
    expect(pageHasMore(5, 5)).toBe(true);
    expect(pageHasMore(4, 5)).toBe(false);
  });
});
