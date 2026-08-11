import { describe, expect, it } from "vitest";
import { AppRoute } from "@/domain/navigation/route";

describe("AppRoute", () => {
  it("maps pathnames to routes", () => {
    expect(AppRoute.fromPathname("/app/")).toEqual({ kind: "Home" });
    expect(AppRoute.fromPathname("/app/notifications")).toEqual({ kind: "Notifications" });
    expect(AppRoute.fromPathname("/app/search")).toEqual({ kind: "Search" });
  });

  it("round-trips through toPath", () => {
    const route = AppRoute.notifications();
    expect(AppRoute.fromPathname(AppRoute.toPath(route))).toEqual(route);
  });
});
