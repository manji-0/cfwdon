/** Mastodon-style `g` then key navigation. */
export const GoChord = {
  timeoutMs: 1000,

  pathFor: (key: string): string | null => {
    switch (key.toLowerCase()) {
      case "h":
        return "/";
      case "n":
        return "/notifications";
      case "s":
        return "/search";
      case "e":
        return "/explore";
      case "p":
        return "/profile";
      case "t":
        return "/public/local";
      case "f":
        return "/public";
      case "b":
        return "/bookmarks";
      case "l":
        return "/lists";
      case "c":
        return "/settings";
      default:
        return null;
    }
  },
} as const;
