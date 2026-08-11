export type AppRoute =
  | Readonly<{ kind: "Home" }>
  | Readonly<{ kind: "Notifications" }>
  | Readonly<{ kind: "Search" }>
  | Readonly<{ kind: "Profile" }>
  | Readonly<{ kind: "Settings" }>
  | Readonly<{ kind: "Bookmarks" }>
  | Readonly<{ kind: "Lists" }>
  | Readonly<{ kind: "Messages" }>;

export const AppRoute = {
  home: (): AppRoute => ({ kind: "Home" }),
  notifications: (): AppRoute => ({ kind: "Notifications" }),
  search: (): AppRoute => ({ kind: "Search" }),
  profile: (): AppRoute => ({ kind: "Profile" }),
  settings: (): AppRoute => ({ kind: "Settings" }),
  bookmarks: (): AppRoute => ({ kind: "Bookmarks" }),
  lists: (): AppRoute => ({ kind: "Lists" }),
  messages: (): AppRoute => ({ kind: "Messages" }),

  fromPathname: (pathname: string): AppRoute => {
    const normalized = pathname
      .replace(/^\/app\/?/, "")
      .replace(/^\/+/, "")
      .replace(/\/$/, "");
    switch (normalized) {
      case "":
        return AppRoute.home();
      case "notifications":
        return AppRoute.notifications();
      case "search":
        return AppRoute.search();
      case "profile":
        return AppRoute.profile();
      case "settings":
        return AppRoute.settings();
      case "bookmarks":
        return AppRoute.bookmarks();
      case "lists":
        return AppRoute.lists();
      case "messages":
        return AppRoute.messages();
      default:
        return AppRoute.home();
    }
  },

  toPath: (route: AppRoute): string => {
    switch (route.kind) {
      case "Home":
        return "/";
      case "Notifications":
        return "/notifications";
      case "Search":
        return "/search";
      case "Profile":
        return "/profile";
      case "Settings":
        return "/settings";
      case "Bookmarks":
        return "/bookmarks";
      case "Lists":
        return "/lists";
      case "Messages":
        return "/messages";
    }
  },

  label: (route: AppRoute): string => {
    switch (route.kind) {
      case "Home":
        return "ホーム";
      case "Notifications":
        return "通知";
      case "Search":
        return "検索";
      case "Profile":
        return "プロフィール";
      case "Settings":
        return "設定";
      case "Bookmarks":
        return "ブックマーク";
      case "Lists":
        return "リスト";
      case "Messages":
        return "メッセージ";
    }
  },
} as const;

export const assertNever = (value: never): never => {
  throw new Error(`unexpected value: ${JSON.stringify(value)}`);
};
