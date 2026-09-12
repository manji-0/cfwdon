import { AppRoute } from "@/domain/navigation/route";

/** Mastodon-style `g` then key navigation. */
export const GoChord = {
  timeoutMs: 1000,

  routeFor: (key: string): AppRoute | null => {
    switch (key.toLowerCase()) {
      case "h":
        return AppRoute.home();
      case "n":
        return AppRoute.notifications();
      case "s":
        return AppRoute.search();
      case "p":
        return AppRoute.profile();
      case "t":
        return AppRoute.publicTimeline(true);
      case "f":
        return AppRoute.publicTimeline(false);
      case "b":
        return AppRoute.bookmarks();
      case "l":
        return AppRoute.lists();
      case "c":
        return AppRoute.settings();
      default:
        return null;
    }
  },

  pathFor: (key: string): string | null => {
    const route = GoChord.routeFor(key);
    return route ? AppRoute.toPath(route) : null;
  },
} as const;
