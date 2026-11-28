import { describe, expect, it } from "vitest";
import { nextMaxIdFromLinkHeader } from "@/infrastructure/http/mastodon-fetch";

describe("nextMaxIdFromLinkHeader", () => {
  it("reads max_id from the rel=next entry", () => {
    const header =
      '<https://x.test/api/v1/favourites?limit=20&max_id=17>; rel="next", ' +
      '<https://x.test/api/v1/favourites?limit=20&min_id=30>; rel="prev"';
    expect(nextMaxIdFromLinkHeader(header)).toBe("17");
  });

  it("finds rel=next regardless of order and quoting", () => {
    const header = "<https://x.test/a?min_id=9>; rel=prev, </a?max_id=5&limit=2>; rel=next";
    expect(nextMaxIdFromLinkHeader(header)).toBe("5");
  });

  it("returns null without a next link or a header", () => {
    expect(nextMaxIdFromLinkHeader(null)).toBeNull();
    expect(nextMaxIdFromLinkHeader("")).toBeNull();
    expect(nextMaxIdFromLinkHeader('<https://x.test/a?min_id=9>; rel="prev"')).toBeNull();
  });

  it("returns null when the next link has no max_id", () => {
    expect(nextMaxIdFromLinkHeader('<https://x.test/a?limit=2>; rel="next"')).toBeNull();
  });
});
