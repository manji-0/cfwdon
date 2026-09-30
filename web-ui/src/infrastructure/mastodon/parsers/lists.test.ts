import { describe, expect, it } from "vitest";
import { isArkError } from "@/infrastructure/mastodon/parse";
import { parseAccountList } from "@/infrastructure/mastodon/parsers/lists";

describe("parseAccountList", () => {
  it("maps list documents to domain lists", () => {
    const result = parseAccountList({
      id: "list-1",
      title: "Friends",
      replies_policy: "followed",
      exclusive: true,
    });
    expect(isArkError(result)).toBe(false);
    if (isArkError(result)) {
      return;
    }
    expect(result).toEqual({
      id: "list-1",
      title: "Friends",
      repliesPolicy: "followed",
      exclusive: true,
    });
  });
});
