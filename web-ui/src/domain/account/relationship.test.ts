import { describe, expect, it } from "vitest";
import { Relationship } from "@/domain/account/relationship";

describe("Relationship.followLabel", () => {
  it("labels an idle relationship as follow", () => {
    expect(Relationship.followLabel(Relationship.empty("1"), false)).toBe("フォロー");
  });

  it("asks to request follow when the account is locked", () => {
    expect(Relationship.followLabel(Relationship.empty("1"), true)).toBe("フォローをリクエスト");
  });

  it("shows following and requested states", () => {
    expect(
      Relationship.followLabel({ ...Relationship.empty("1"), following: true }, false),
    ).toBe("フォロー中");
    expect(
      Relationship.followLabel({ ...Relationship.empty("1"), requested: true }, true),
    ).toBe("リクエスト中");
  });
});
