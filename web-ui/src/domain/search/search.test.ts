import { describe, expect, it } from "vitest";
import { SearchType } from "@/domain/search/search";

describe("SearchType", () => {
  it("parses URL params", () => {
    expect(SearchType.fromParam(null)).toBe("all");
    expect(SearchType.fromParam("accounts")).toBe("accounts");
    expect(SearchType.fromParam("nope")).toBe("all");
  });

  it("resolves URLs and account handles", () => {
    expect(SearchType.shouldResolve("https://example.com/@alice")).toBe(true);
    expect(SearchType.shouldResolve("alice@example.com")).toBe(true);
    expect(SearchType.shouldResolve("@alice")).toBe(true);
    expect(SearchType.shouldResolve("hello world")).toBe(false);
  });
});
