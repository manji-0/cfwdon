import { describe, expect, it } from "vitest";
import {
  FOREGROUND_CATCH_UP_AFTER_MS,
  FOREGROUND_LIST_REFRESH_MAX_SCROLL_Y,
  ForegroundResume,
} from "@/domain/cache/foreground-resume";
import { VIEW_CACHE_REMOUNT_SKIP_MS } from "@/domain/cache/view-readiness";

describe("ForegroundResume.forHiddenDuration", () => {
  it("keeps a live stream after a short background", () => {
    expect(ForegroundResume.forHiddenDuration(0)).toEqual(ForegroundResume.keepStream());
    expect(ForegroundResume.forHiddenDuration(FOREGROUND_CATCH_UP_AFTER_MS - 1)).toEqual(
      ForegroundResume.keepStream(),
    );
  });

  it("catches up after a long background", () => {
    expect(ForegroundResume.forHiddenDuration(FOREGROUND_CATCH_UP_AFTER_MS)).toEqual(
      ForegroundResume.catchUp(),
    );
  });
});

describe("ForegroundResume.forBfcacheRestore", () => {
  it("always catches up", () => {
    expect(ForegroundResume.forBfcacheRestore()).toEqual(ForegroundResume.catchUp());
  });
});

describe("ForegroundResume.shouldEmit", () => {
  it("emits the first catch-up and suppresses a duplicate inside the remount window", () => {
    expect(ForegroundResume.shouldEmit(null, 1_000)).toBe(true);
    expect(ForegroundResume.shouldEmit(1_000, 1_000 + VIEW_CACHE_REMOUNT_SKIP_MS - 1)).toBe(false);
    expect(ForegroundResume.shouldEmit(1_000, 1_000 + VIEW_CACHE_REMOUNT_SKIP_MS)).toBe(true);
  });
});

describe("ForegroundResume.shouldRefreshVisibleList", () => {
  it("refreshes only when the viewport is near the top", () => {
    expect(ForegroundResume.shouldRefreshVisibleList(0)).toBe(true);
    expect(ForegroundResume.shouldRefreshVisibleList(FOREGROUND_LIST_REFRESH_MAX_SCROLL_Y)).toBe(
      true,
    );
    expect(
      ForegroundResume.shouldRefreshVisibleList(FOREGROUND_LIST_REFRESH_MAX_SCROLL_Y + 1),
    ).toBe(false);
  });
});
