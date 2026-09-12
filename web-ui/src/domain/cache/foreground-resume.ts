import { VIEW_CACHE_REMOUNT_SKIP_MS } from "@/domain/cache/view-readiness";

/** Hidden longer than this: treat the tab as stale (force stream reconnect + REST catch-up). */
export const FOREGROUND_CATCH_UP_AFTER_MS = 30_000;
/** Skip replacing a scrolled list so a catch-up fetch does not steal the reading position. */
export const FOREGROUND_LIST_REFRESH_MAX_SCROLL_Y = 80;

export type KeepStream = Readonly<{ kind: "KeepStream" }>;
export type CatchUp = Readonly<{ kind: "CatchUp" }>;

/** What to do when a backgrounded tab becomes visible again. */
export type ForegroundResume = KeepStream | CatchUp;

export const ForegroundResume = {
  keepStream: (): KeepStream => ({ kind: "KeepStream" }),
  catchUp: (): CatchUp => ({ kind: "CatchUp" }),

  forHiddenDuration: (hiddenMs: number): ForegroundResume =>
    hiddenMs >= FOREGROUND_CATCH_UP_AFTER_MS
      ? ForegroundResume.catchUp()
      : ForegroundResume.keepStream(),

  /** Back-forward cache restore always misses the live socket. */
  forBfcacheRestore: (): CatchUp => ForegroundResume.catchUp(),

  shouldEmit: (lastEmittedAt: number | null, now: number): boolean =>
    lastEmittedAt === null || now - lastEmittedAt >= VIEW_CACHE_REMOUNT_SKIP_MS,

  shouldRefreshVisibleList: (scrollY: number): boolean =>
    scrollY <= FOREGROUND_LIST_REFRESH_MAX_SCROLL_Y,
} as const;
