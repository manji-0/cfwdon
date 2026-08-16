import { describe, expect, it } from "vitest";
import { appendConversationStatus, flattenConversationStatuses } from "@/domain/conversations/thread";
import type { Status } from "@/domain/status/status";
import { Visibility } from "@/domain/status/visibility";

const status = (id: string, inReplyToId: string | null, visibility = Visibility.direct()): Status => ({
  id,
  createdAt: "2026-01-01T00:00:00.000Z",
  content: `<p>${id}</p>`,
  spoilerText: "",
  sensitive: false,
  visibility,
  inReplyToId,
  repliesCount: 0,
  reblogsCount: 0,
  favouritesCount: 0,
  favourited: false,
  reblogged: false,
  bookmarked: false,
  account: {
    id: "acct-1",
    username: "alice",
    acct: "alice",
    displayName: "Alice",
    avatar: "https://example.test/a.png",
  },
  mediaAttachments: [],
  card: null,
  reblog: null,
});

describe("flattenConversationStatuses", () => {
  it("orders ancestors, focus, descendants", () => {
    const ids = flattenConversationStatuses(
      [status("a", null)],
      status("b", "a"),
      [status("c", "b")],
    ).map((item) => item.id);
    expect(ids).toEqual(["a", "b", "c"]);
  });
});

describe("appendConversationStatus", () => {
  it("appends a direct reply in the same thread", () => {
    const current = [status("a", null)];
    const next = appendConversationStatus(current, status("b", "a"));
    expect(next.map((item) => item.id)).toEqual(["a", "b"]);
  });

  it("ignores public statuses and duplicates", () => {
    const current = [status("a", null)];
    expect(appendConversationStatus(current, status("x", "a", Visibility.public()))).toBe(current);
    expect(appendConversationStatus(current, status("a", null))).toBe(current);
  });
});
