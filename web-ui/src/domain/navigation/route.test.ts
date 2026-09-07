import { describe, expect, it } from "vitest";
import { AppRoute } from "@/domain/navigation/route";

describe("AppRoute", () => {
  it("maps pathnames to routes", () => {
    expect(AppRoute.fromPathname("/app/")).toEqual({ kind: "Home" });
    expect(AppRoute.fromPathname("/app/notifications")).toEqual({ kind: "Notifications" });
    expect(AppRoute.fromPathname("/notifications")).toEqual({ kind: "Notifications" });
    expect(AppRoute.fromPathname("/app/search")).toEqual({ kind: "Search" });
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
  });

  it("round-trips through toPath", () => {
    for (const route of [
      AppRoute.notifications(),
      AppRoute.bookmarks(),
      AppRoute.favourites(),
      AppRoute.scheduled(),
      AppRoute.publicTimeline(true),
      AppRoute.tag("fediverse"),
      AppRoute.lists(),
      AppRoute.messages(),
      AppRoute.newMessage(),
      AppRoute.conversation("conv-1"),
    ]) {
      expect(AppRoute.fromPathname(AppRoute.toPath(route))).toEqual(route);
    }
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
    expect(AppRoute.isMePath("/profile/other", "acct-1")).toBe(false);
  });
});
