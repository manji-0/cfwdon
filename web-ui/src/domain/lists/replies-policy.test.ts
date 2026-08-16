import { describe, expect, it } from "vitest";
import { ListRepliesPolicy } from "@/domain/lists/replies-policy";
import { Status } from "@/domain/status/status";
import { isArkError } from "@/infrastructure/mastodon/parse";
import { parseAccountList } from "@/infrastructure/mastodon/parsers/lists";
import { parseStatus } from "@/infrastructure/mastodon/parsers/status";

describe("ListRepliesPolicy", () => {
  it("normalizes known API values and falls back for unknown ones", () => {
    expect(ListRepliesPolicy.fromApi("followed")).toBe("followed");
    expect(ListRepliesPolicy.fromApi("LIST")).toBe("list");
    expect(ListRepliesPolicy.fromApi("none")).toBe("none");
    expect(ListRepliesPolicy.fromApi("all")).toBe("list");
  });
});

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

describe("parseStatus bookmarked", () => {
  const baseStatus = {
    id: "status-1",
    created_at: "2026-08-11T00:00:00.000Z",
    content: "<p>hello</p>",
    visibility: "public",
    account: {
      id: "acct-1",
      username: "alice",
      acct: "alice",
      display_name: "Alice",
      avatar: "https://example.com/a.png",
    },
  } as const;

  it("defaults bookmarked to false when missing", () => {
    const result = parseStatus(baseStatus);
    expect(isArkError(result)).toBe(false);
    if (isArkError(result)) {
      return;
    }
    expect(Status.displayBody(result).bookmarked).toBe(false);
  });

  it("parses bookmarked true", () => {
    const result = parseStatus({ ...baseStatus, bookmarked: true });
    expect(isArkError(result)).toBe(false);
    if (isArkError(result)) {
      return;
    }
    expect(Status.displayBody(result).bookmarked).toBe(true);
  });
});
