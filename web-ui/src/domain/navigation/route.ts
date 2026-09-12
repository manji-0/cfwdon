import { SearchType } from "@/domain/search/search";

export type AppRoute =
  | Readonly<{ kind: "Home" }>
  | Readonly<{ kind: "PublicTimeline"; local: boolean }>
  | Readonly<{ kind: "Tag"; name: string }>
  | Readonly<{ kind: "Notifications" }>
  | Readonly<{ kind: "Search"; query: string; type: SearchType }>
  | Readonly<{ kind: "Profile" }>
  | Readonly<{ kind: "Account"; accountId: string }>
  | Readonly<{ kind: "AccountFollowers"; accountId: string }>
  | Readonly<{ kind: "AccountFollowing"; accountId: string }>
  | Readonly<{ kind: "Settings" }>
  | Readonly<{ kind: "Bookmarks" }>
  | Readonly<{ kind: "Favourites" }>
  | Readonly<{ kind: "Scheduled" }>
  | Readonly<{ kind: "Lists" }>
  | Readonly<{ kind: "Messages" }>
  | Readonly<{ kind: "NewMessage" }>
  | Readonly<{ kind: "Conversation"; conversationId: string }>
  | Readonly<{ kind: "Status"; statusId: string }>
  | Readonly<{ kind: "StatusHistory"; statusId: string }>
  | Readonly<{ kind: "StatusFavouritedBy"; statusId: string }>
  | Readonly<{ kind: "StatusRebloggedBy"; statusId: string }>
  | Readonly<{ kind: "StatusQuotes"; statusId: string }>;

const normalizePath = (pathname: string): string =>
  pathname
    .replace(/^\/app\/?/, "")
    .replace(/^\/+/, "")
    .replace(/\/$/, "");

const splitHref = (href: string): Readonly<{ pathname: string; search: string }> => {
  const queryIndex = href.indexOf("?");
  if (queryIndex === -1) {
    return { pathname: href, search: "" };
  }
  return { pathname: href.slice(0, queryIndex), search: href.slice(queryIndex + 1) };
};

const searchParams = (query: string, type: SearchType): URLSearchParams => {
  const params = new URLSearchParams();
  if (query !== "") {
    params.set("q", query);
  }
  if (type !== "all") {
    params.set("type", type);
  }
  return params;
};

export type AppLinkTarget = Readonly<{
  to: string;
  params?: Readonly<Record<string, string>>;
  search?: Readonly<{ q?: string; type?: Exclude<SearchType, "all"> }>;
}>;

export const AppRoute = {
  basename: "/app",
  loginHref: "/app/login",
  logoutHref: "/app/logout",

  /** TanStack Router `path` values, in match order. */
  path: {
    home: "/",
    publicTimeline: "/public",
    publicTimelineLocal: "/public/local",
    tag: "/tags/$tagName",
    explore: "/explore",
    statusHistory: "/status/$statusId/history",
    statusFavouritedBy: "/status/$statusId/favourited-by",
    statusRebloggedBy: "/status/$statusId/reblogged-by",
    statusQuotes: "/status/$statusId/quotes",
    status: "/status/$statusId",
    profile: "/profile",
    accountFollowers: "/profile/$accountId/followers",
    accountFollowing: "/profile/$accountId/following",
    account: "/profile/$accountId",
    notifications: "/notifications",
    search: "/search",
    settings: "/settings",
    bookmarks: "/bookmarks",
    favourites: "/favourites",
    scheduled: "/scheduled",
    lists: "/lists",
    messages: "/messages",
    newMessage: "/messages/new",
    conversation: "/messages/$conversationId",
  },

  home: (): AppRoute => ({ kind: "Home" }),
  publicTimeline: (local = false): AppRoute => ({ kind: "PublicTimeline", local }),
  tag: (name: string): AppRoute => ({ kind: "Tag", name }),
  notifications: (): AppRoute => ({ kind: "Notifications" }),
  search: (query = "", type: SearchType = "all"): AppRoute => ({ kind: "Search", query, type }),
  profile: (): AppRoute => ({ kind: "Profile" }),
  account: (accountId: string): AppRoute => ({ kind: "Account", accountId }),
  accountFollowers: (accountId: string): AppRoute => ({ kind: "AccountFollowers", accountId }),
  accountFollowing: (accountId: string): AppRoute => ({ kind: "AccountFollowing", accountId }),
  settings: (): AppRoute => ({ kind: "Settings" }),
  bookmarks: (): AppRoute => ({ kind: "Bookmarks" }),
  favourites: (): AppRoute => ({ kind: "Favourites" }),
  scheduled: (): AppRoute => ({ kind: "Scheduled" }),
  lists: (): AppRoute => ({ kind: "Lists" }),
  messages: (): AppRoute => ({ kind: "Messages" }),
  newMessage: (): AppRoute => ({ kind: "NewMessage" }),
  conversation: (conversationId: string): AppRoute => ({ kind: "Conversation", conversationId }),
  status: (statusId: string): AppRoute => ({ kind: "Status", statusId }),
  statusHistory: (statusId: string): AppRoute => ({ kind: "StatusHistory", statusId }),
  statusFavouritedBy: (statusId: string): AppRoute => ({ kind: "StatusFavouritedBy", statusId }),
  statusRebloggedBy: (statusId: string): AppRoute => ({ kind: "StatusRebloggedBy", statusId }),
  statusQuotes: (statusId: string): AppRoute => ({ kind: "StatusQuotes", statusId }),

  fromPathname: (pathname: string): AppRoute => {
    const { pathname: pathPart, search } = splitHref(pathname);
    const normalized = normalizePath(pathPart);
    const params = new URLSearchParams(search);
    const [head, ...rest] = normalized.split("/");
    switch (head) {
      case "":
        return AppRoute.home();
      case "public":
        return AppRoute.publicTimeline(rest[0] === "local");
      case "tags":
        return rest[0] ? AppRoute.tag(decodeURIComponent(rest[0])) : AppRoute.home();
      case "explore":
        return AppRoute.home();
      case "notifications":
        return AppRoute.notifications();
      case "search":
        return AppRoute.search(params.get("q") ?? "", SearchType.fromParam(params.get("type")));
      case "profile": {
        const accountId = rest[0];
        if (!accountId) {
          return AppRoute.profile();
        }
        if (rest.length === 2 && rest[1] === "followers") {
          return AppRoute.accountFollowers(accountId);
        }
        if (rest.length === 2 && rest[1] === "following") {
          return AppRoute.accountFollowing(accountId);
        }
        return AppRoute.account(accountId);
      }
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
      case "status": {
        const statusId = rest[0];
        if (!statusId) {
          return AppRoute.home();
        }
        if (rest.length === 2 && rest[1] === "history") {
          return AppRoute.statusHistory(statusId);
        }
        if (rest.length === 2 && rest[1] === "favourited-by") {
          return AppRoute.statusFavouritedBy(statusId);
        }
        if (rest.length === 2 && rest[1] === "reblogged-by") {
          return AppRoute.statusRebloggedBy(statusId);
        }
        if (rest.length === 2 && rest[1] === "quotes") {
          return AppRoute.statusQuotes(statusId);
        }
        return AppRoute.status(statusId);
      }
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
      case "Notifications":
        return "/notifications";
      case "Search": {
        const query = searchParams(route.query, route.type).toString();
        return query.length > 0 ? `/search?${query}` : "/search";
      }
      case "Profile":
        return "/profile";
      case "Account":
        return `/profile/${route.accountId}`;
      case "AccountFollowers":
        return `/profile/${route.accountId}/followers`;
      case "AccountFollowing":
        return `/profile/${route.accountId}/following`;
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
      case "Status":
        return `/status/${route.statusId}`;
      case "StatusHistory":
        return `/status/${route.statusId}/history`;
      case "StatusFavouritedBy":
        return `/status/${route.statusId}/favourited-by`;
      case "StatusRebloggedBy":
        return `/status/${route.statusId}/reblogged-by`;
      case "StatusQuotes":
        return `/status/${route.statusId}/quotes`;
    }
  },

  toSearchParams: (query: string, type: SearchType = "all"): URLSearchParams =>
    searchParams(query, type),

  toLink: (route: AppRoute): AppLinkTarget => {
    switch (route.kind) {
      case "Home":
        return { to: AppRoute.path.home };
      case "PublicTimeline":
        return {
          to: route.local ? AppRoute.path.publicTimelineLocal : AppRoute.path.publicTimeline,
        };
      case "Tag":
        return { to: AppRoute.path.tag, params: { tagName: route.name } };
      case "Notifications":
        return { to: AppRoute.path.notifications };
      case "Search":
        return {
          to: AppRoute.path.search,
          search: {
            q: route.query === "" ? undefined : route.query,
            type: route.type === "all" ? undefined : route.type,
          },
        };
      case "Profile":
        return { to: AppRoute.path.profile };
      case "Account":
        return { to: AppRoute.path.account, params: { accountId: route.accountId } };
      case "AccountFollowers":
        return { to: AppRoute.path.accountFollowers, params: { accountId: route.accountId } };
      case "AccountFollowing":
        return { to: AppRoute.path.accountFollowing, params: { accountId: route.accountId } };
      case "Settings":
        return { to: AppRoute.path.settings };
      case "Bookmarks":
        return { to: AppRoute.path.bookmarks };
      case "Favourites":
        return { to: AppRoute.path.favourites };
      case "Scheduled":
        return { to: AppRoute.path.scheduled };
      case "Lists":
        return { to: AppRoute.path.lists };
      case "Messages":
        return { to: AppRoute.path.messages };
      case "NewMessage":
        return { to: AppRoute.path.newMessage };
      case "Conversation":
        return { to: AppRoute.path.conversation, params: { conversationId: route.conversationId } };
      case "Status":
        return { to: AppRoute.path.status, params: { statusId: route.statusId } };
      case "StatusHistory":
        return { to: AppRoute.path.statusHistory, params: { statusId: route.statusId } };
      case "StatusFavouritedBy":
        return { to: AppRoute.path.statusFavouritedBy, params: { statusId: route.statusId } };
      case "StatusRebloggedBy":
        return { to: AppRoute.path.statusRebloggedBy, params: { statusId: route.statusId } };
      case "StatusQuotes":
        return { to: AppRoute.path.statusQuotes, params: { statusId: route.statusId } };
    }
  },

  absoluteHref: (route: AppRoute): string => `${AppRoute.basename}${AppRoute.toPath(route)}`,

  isInboxPath: (pathname: string): boolean => {
    const normalized = normalizePath(splitHref(pathname).pathname);
    return normalized === "notifications" || normalized.startsWith("messages");
  },

  isMePath: (pathname: string, selfAccountId: string | null): boolean => {
    const normalized = normalizePath(splitHref(pathname).pathname);
    if (
      normalized === "profile" ||
      normalized === "settings" ||
      normalized === "bookmarks" ||
      normalized === "scheduled" ||
      normalized === "lists" ||
      normalized === "favourites"
    ) {
      return true;
    }
    if (!selfAccountId) {
      return false;
    }
    return (
      normalized === `profile/${selfAccountId}` ||
      normalized.startsWith(`profile/${selfAccountId}/`)
    );
  },

  isLocalPublicTimeline: (pathname: string): boolean => {
    const route = AppRoute.fromPathname(pathname);
    return route.kind === "PublicTimeline" && route.local;
  },

  label: (route: AppRoute): string => {
    switch (route.kind) {
      case "Home":
        return "ホーム";
      case "PublicTimeline":
        return route.local ? "ローカル" : "連合";
      case "Tag":
        return `#${route.name}`;
      case "Notifications":
        return "受信";
      case "Search":
        return "検索";
      case "Profile":
        return "自分";
      case "Account":
        return "プロフィール";
      case "AccountFollowers":
        return "フォロワー";
      case "AccountFollowing":
        return "フォロー中";
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
      case "Status":
        return "スレッド";
      case "StatusHistory":
        return "編集履歴";
      case "StatusFavouritedBy":
        return "いいねした人";
      case "StatusRebloggedBy":
        return "ブーストした人";
      case "StatusQuotes":
        return "引用";
    }
  },
} as const;
