import { describe, expect, it } from "vitest";
import { GoChord } from "@/domain/navigation/go-chord";

describe("GoChord", () => {
  it("maps known keys to app paths", () => {
    expect(GoChord.pathFor("h")).toBe("/");
    expect(GoChord.pathFor("N")).toBe("/notifications");
    expect(GoChord.pathFor("e")).toBe("/explore");
    expect(GoChord.pathFor("t")).toBe("/public/local");
    expect(GoChord.pathFor("f")).toBe("/public");
    expect(GoChord.pathFor("c")).toBe("/settings");
    expect(GoChord.pathFor("p")).toBe("/profile");
    expect(GoChord.pathFor("s")).toBe("/search");
    expect(GoChord.pathFor("b")).toBe("/bookmarks");
    expect(GoChord.pathFor("l")).toBe("/lists");
  });

  it("returns null for unknown keys", () => {
    expect(GoChord.pathFor("x")).toBeNull();
    expect(GoChord.pathFor("g")).toBeNull();
  });
});
