import { describe, expect, it } from "vitest";
import { ListRepliesPolicy } from "@/domain/lists/replies-policy";

describe("ListRepliesPolicy", () => {
  it("normalizes known API values and falls back for unknown ones", () => {
    expect(ListRepliesPolicy.fromApi("followed")).toBe("followed");
    expect(ListRepliesPolicy.fromApi("LIST")).toBe("list");
    expect(ListRepliesPolicy.fromApi("none")).toBe("none");
    expect(ListRepliesPolicy.fromApi("all")).toBe("list");
  });
});
