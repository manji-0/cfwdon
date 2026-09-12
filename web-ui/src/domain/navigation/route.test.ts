import { describe, expect, it } from "vitest";
import { AppRoute } from "@/domain/navigation/route";
import routerSource from "@/ui/router.tsx?raw";

describe("AppRoute", () => {
  it("maps pathnames to routes", () => {
    expect(AppRoute.fromPathname("/app/")).toEqual({ kind: "Home" });
    expect(AppRoute.fromPathname("/app/notifications")).toEqual({ kind: "Notifications" });
    expect(AppRoute.fromPathname("/notifications")).toEqual({ kind: "Notifications" });
    expect(AppRoute.fromPathname("/app/search")).toEqual({
      kind: "Search",
      query: "",
      type: "all",
    });
    expect(AppRoute.fromPathname("/search?q=@alice&type=accounts")).toEqual({
      kind: "Search",
      query: "@alice",
      type: "accounts",
    });
    expect(AppRoute.fromPathname("/bookmarks")).toEqual({ kind: "Bookmarks" });
    expect(AppRoute.fromPathname("/favourites")).toEqual({ kind: "Favourites" });
    expect(AppRoute.fromPathname("/scheduled")).toEqual({ kind: "Scheduled" });
    expect(AppRoute.fromPathname("/public")).toEqual({ kind: "PublicTimeline", local: false });
    expect(AppRoute.fromPathname("/public/local")).toEqual({ kind: "PublicTimeline", local: true });
    expect(AppRoute.fromPathname("/tags/fediverse")).toEqual({ kind: "Tag", name: "fediverse" });
    expect(AppRoute.fromPathname("/explore")).toEqual({ kind: "Home" });
    expect(AppRoute.fromPathname("/lists")).toEqual({ kind: "Lists" });
    expect(AppRoute.fromPathname("/messages")).toEqual({ kind: "Messages" });
    expect(AppRoute.fromPathname("/messages/new")).toEqual({ kind: "NewMessage" });
    expect(AppRoute.fromPathname("/messages/conv-1")).toEqual({
      kind: "Conversation",
      conversationId: "conv-1",
    });
    expect(AppRoute.fromPathname("/status/s1")).toEqual({ kind: "Status", statusId: "s1" });
    expect(AppRoute.fromPathname("/status/s1/history")).toEqual({
      kind: "StatusHistory",
      statusId: "s1",
    });
    expect(AppRoute.fromPathname("/status/s1/favourited-by")).toEqual({
      kind: "StatusFavouritedBy",
      statusId: "s1",
    });
    expect(AppRoute.fromPathname("/status/s1/reblogged-by")).toEqual({
      kind: "StatusRebloggedBy",
      statusId: "s1",
    });
    expect(AppRoute.fromPathname("/status/s1/quotes")).toEqual({
      kind: "StatusQuotes",
      statusId: "s1",
    });
    expect(AppRoute.fromPathname("/profile")).toEqual({ kind: "Profile" });
    expect(AppRoute.fromPathname("/profile/acct-1")).toEqual({
      kind: "Account",
      accountId: "acct-1",
    });
    expect(AppRoute.fromPathname("/profile/acct-1/followers")).toEqual({
      kind: "AccountFollowers",
      accountId: "acct-1",
    });
    expect(AppRoute.fromPathname("/profile/acct-1/following")).toEqual({
      kind: "AccountFollowing",
      accountId: "acct-1",
    });
  });

  it("round-trips through toPath", () => {
    for (const route of [
      AppRoute.home(),
      AppRoute.notifications(),
      AppRoute.search(),
      AppRoute.search("@alice", "accounts"),
      AppRoute.bookmarks(),
      AppRoute.favourites(),
      AppRoute.scheduled(),
      AppRoute.publicTimeline(true),
      AppRoute.tag("fediverse"),
      AppRoute.lists(),
      AppRoute.messages(),
      AppRoute.newMessage(),
      AppRoute.conversation("conv-1"),
      AppRoute.profile(),
      AppRoute.account("acct-1"),
      AppRoute.accountFollowers("acct-1"),
      AppRoute.accountFollowing("acct-1"),
      AppRoute.status("s1"),
      AppRoute.statusHistory("s1"),
      AppRoute.statusFavouritedBy("s1"),
      AppRoute.statusRebloggedBy("s1"),
      AppRoute.statusQuotes("s1"),
      AppRoute.settings(),
    ]) {
      expect(AppRoute.fromPathname(AppRoute.toPath(route))).toEqual(route);
    }
  });

  it("builds absolute hrefs under /app", () => {
    expect(AppRoute.absoluteHref(AppRoute.search("@alice"))).toBe("/app/search?q=%40alice");
    expect(AppRoute.loginHref).toBe("/app/login");
    expect(AppRoute.logoutHref).toBe("/app/logout");
  });

  it("treats notifications and messages as the inbox hub", () => {
    expect(AppRoute.isInboxPath("/notifications")).toBe(true);
    expect(AppRoute.isInboxPath("/messages")).toBe(true);
    expect(AppRoute.isInboxPath("/messages/new")).toBe(true);
    expect(AppRoute.isInboxPath("/search")).toBe(false);
  });

  it("treats self surfaces as the me hub", () => {
    expect(AppRoute.isMePath("/profile", "acct-1")).toBe(true);
    expect(AppRoute.isMePath("/settings", "acct-1")).toBe(true);
    expect(AppRoute.isMePath("/bookmarks", "acct-1")).toBe(true);
    expect(AppRoute.isMePath("/profile/acct-1", "acct-1")).toBe(true);
    expect(AppRoute.isMePath("/profile/acct-1/followers", "acct-1")).toBe(true);
    expect(AppRoute.isMePath("/profile/other", "acct-1")).toBe(false);
  });

  it("round-trips through toLink", () => {
    expect(AppRoute.toLink(AppRoute.home())).toEqual({ to: AppRoute.path.home });
    expect(AppRoute.toLink(AppRoute.status("s1"))).toEqual({
      to: AppRoute.path.status,
      params: { statusId: "s1" },
    });
    expect(AppRoute.toLink(AppRoute.search("hello", "accounts"))).toEqual({
      to: AppRoute.path.search,
      search: { q: "hello", type: "accounts" },
    });
    expect(AppRoute.toLink(AppRoute.search())).toEqual({
      to: AppRoute.path.search,
      search: { q: "", type: "all" },
    });
  });

  it("keeps router.tsx paths on AppRoute.path", () => {
    for (const key of Object.keys(AppRoute.path)) {
      expect(routerSource).toContain(`AppRoute.path.${key}`);
    }
    const pathLiterals = [...routerSource.matchAll(/path:\s*"([^"]*)"/g)].map((match) => match[1]);
    expect(pathLiterals).toEqual([]);
    expect(routerSource).toContain("lazy-pages");
  });
});
