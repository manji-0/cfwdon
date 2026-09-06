export type AppRoute =
  | Readonly<{ kind: "Home" }>
  | Readonly<{ kind: "PublicTimeline"; local: boolean }>
  | Readonly<{ kind: "Tag"; name: string }>
  | Readonly<{ kind: "Explore" }>
  | Readonly<{ kind: "Notifications" }>
  | Readonly<{ kind: "Search" }>
  | Readonly<{ kind: "Profile" }>
  | Readonly<{ kind: "Settings" }>
  | Readonly<{ kind: "Bookmarks" }>
  | Readonly<{ kind: "Favourites" }>
  | Readonly<{ kind: "Scheduled" }>
  | Readonly<{ kind: "Lists" }>
  | Readonly<{ kind: "Messages" }>
  | Readonly<{ kind: "NewMessage" }>
  | Readonly<{ kind: "Conversation"; conversationId: string }>;

export const AppRoute = {
  home: (): AppRoute => ({ kind: "Home" }),
  publicTimeline: (local = false): AppRoute => ({ kind: "PublicTimeline", local }),
  tag: (name: string): AppRoute => ({ kind: "Tag", name }),
  explore: (): AppRoute => ({ kind: "Explore" }),
  notifications: (): AppRoute => ({ kind: "Notifications" }),
  search: (): AppRoute => ({ kind: "Search" }),
  profile: (): AppRoute => ({ kind: "Profile" }),
  settings: (): AppRoute => ({ kind: "Settings" }),
  bookmarks: (): AppRoute => ({ kind: "Bookmarks" }),
  favourites: (): AppRoute => ({ kind: "Favourites" }),
  scheduled: (): AppRoute => ({ kind: "Scheduled" }),
  lists: (): AppRoute => ({ kind: "Lists" }),
  messages: (): AppRoute => ({ kind: "Messages" }),
  newMessage: (): AppRoute => ({ kind: "NewMessage" }),
  conversation: (conversationId: string): AppRoute => ({ kind: "Conversation", conversationId }),

  fromPathname: (pathname: string): AppRoute => {
    const normalized = pathname
      .replace(/^\/app\/?/, "")
      .replace(/^\/+/, "")
      .replace(/\/$/, "");
    const [head, ...rest] = normalized.split("/");
    switch (head) {
      case "":
        return AppRoute.home();
      case "public":
        return AppRoute.publicTimeline(rest[0] === "local");
      case "tags":
        return rest[0] ? AppRoute.tag(decodeURIComponent(rest[0])) : AppRoute.home();
      case "explore":
        return AppRoute.explore();
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
      case "favourites":
        return AppRoute.favourites();
      case "scheduled":
        return AppRoute.scheduled();
      case "lists":
        return AppRoute.lists();
      case "messages": {
        if (rest.length === 0) {
          return AppRoute.messages();
        }
        if (rest[0] === "new" && rest.length === 1) {
          return AppRoute.newMessage();
        }
        if (rest.length === 1 && rest[0]) {
          return AppRoute.conversation(rest[0]);
        }
        return AppRoute.messages();
      }
      default:
        return AppRoute.home();
    }
  },

  toPath: (route: AppRoute): string => {
    switch (route.kind) {
      case "Home":
        return "/";
      case "PublicTimeline":
        return route.local ? "/public/local" : "/public";
      case "Tag":
        return `/tags/${encodeURIComponent(route.name)}`;
      case "Explore":
        return "/explore";
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
      case "Favourites":
        return "/favourites";
      case "Scheduled":
        return "/scheduled";
      case "Lists":
        return "/lists";
      case "Messages":
        return "/messages";
      case "NewMessage":
        return "/messages/new";
      case "Conversation":
        return `/messages/${route.conversationId}`;
    }
  },

  label: (route: AppRoute): string => {
    switch (route.kind) {
      case "Home":
        return "ホーム";
      case "PublicTimeline":
        return route.local ? "ローカル" : "連合";
      case "Tag":
        return `#${route.name}`;
      case "Explore":
        return "探索";
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
      case "Favourites":
        return "お気に入り";
      case "Scheduled":
        return "予約投稿";
      case "Lists":
        return "リスト";
      case "Messages":
      case "NewMessage":
      case "Conversation":
        return "メッセージ";
    }
  },
} as const;
