export type AppRoute =
  | Readonly<{ kind: "Home" }>
  | Readonly<{ kind: "Notifications" }>
  | Readonly<{ kind: "Search" }>
  | Readonly<{ kind: "Profile" }>
  | Readonly<{ kind: "Settings" }>;

export const AppRoute = {
  home: (): AppRoute => ({ kind: "Home" }),
  notifications: (): AppRoute => ({ kind: "Notifications" }),
  search: (): AppRoute => ({ kind: "Search" }),
  profile: (): AppRoute => ({ kind: "Profile" }),
  settings: (): AppRoute => ({ kind: "Settings" }),

  fromPathname: (pathname: string): AppRoute => {
    const normalized = pathname.replace(/^\/app\/?/, "").replace(/\/$/, "");
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
      default:
        return AppRoute.home();
    }
  },

  toPath: (route: AppRoute): string => {
    switch (route.kind) {
      case "Home":
        return "/app/";
      case "Notifications":
        return "/app/notifications";
      case "Search":
        return "/app/search";
      case "Profile":
        return "/app/profile";
      case "Settings":
        return "/app/settings";
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
    }
  },
} as const;

export const assertNever = (value: never): never => {
  throw new Error(`unexpected value: ${JSON.stringify(value)}`);
};
