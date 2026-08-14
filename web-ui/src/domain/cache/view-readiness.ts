import type { CachedView } from "./cached-view";

/** Skip a duplicate fetch on Strict Mode / instant remount. Streaming views still revalidate after this. */
export const VIEW_CACHE_REMOUNT_SKIP_MS = 2_000;
/** Profile pages have no live stream; skip refetch inside this window. */
export const VIEW_CACHE_PROFILE_FRESH_MS = 30_000;

export type ViewLoad = Readonly<{ kind: "Load" }>;
export type ViewSkip = Readonly<{ kind: "Skip" }>;
export type ViewRevalidate = Readonly<{ kind: "Revalidate" }>;

export type ViewReadiness = ViewLoad | ViewSkip | ViewRevalidate;

type Timestamped = Readonly<{ fetchedAt: number }>;

export const ViewReadiness = {
  load: (): ViewLoad => ({ kind: "Load" }),
  skip: (): ViewSkip => ({ kind: "Skip" }),
  revalidate: (): ViewRevalidate => ({ kind: "Revalidate" }),

  forStreaming: (view: CachedView<Timestamped>, now: number): ViewReadiness => {
    switch (view.kind) {
      case "Absent":
        return ViewReadiness.load();
      case "Present":
        return now - view.value.fetchedAt < VIEW_CACHE_REMOUNT_SKIP_MS
          ? ViewReadiness.skip()
          : ViewReadiness.revalidate();
    }
  },

  forProfile: (view: CachedView<Timestamped>, now: number): ViewReadiness => {
    switch (view.kind) {
      case "Absent":
        return ViewReadiness.load();
      case "Present":
        return now - view.value.fetchedAt < VIEW_CACHE_PROFILE_FRESH_MS
          ? ViewReadiness.skip()
          : ViewReadiness.revalidate();
    }
  },
} as const;
