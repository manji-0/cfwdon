import { describe, expect, it } from "vitest";
import { AppRoute } from "@/domain/navigation/route";

describe("AppRoute", () => {
  it("maps pathnames to routes", () => {
    expect(AppRoute.fromPathname("/app/")).toEqual({ kind: "Home" });
    expect(AppRoute.fromPathname("/app/notifications")).toEqual({ kind: "Notifications" });
    expect(AppRoute.fromPathname("/notifications")).toEqual({ kind: "Notifications" });
    expect(AppRoute.fromPathname("/app/search")).toEqual({ kind: "Search" });
    expect(AppRoute.fromPathname("/bookmarks")).toEqual({ kind: "Bookmarks" });
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
      AppRoute.lists(),
      AppRoute.messages(),
      AppRoute.newMessage(),
      AppRoute.conversation("conv-1"),
    ]) {
      expect(AppRoute.fromPathname(AppRoute.toPath(route))).toEqual(route);
    }
  });
});
