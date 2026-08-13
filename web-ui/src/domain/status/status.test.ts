import { describe, expect, it } from "vitest";
import type { Status } from "@/domain/status/status";
import { Status as StatusModel } from "@/domain/status/status";
import { Visibility } from "@/domain/status/visibility";

const status = (id: string): Status => ({
  id,
  createdAt: "2026-08-13T00:00:00.000Z",
  content: `<p>${id}</p>`,
  spoilerText: "",
  sensitive: false,
  visibility: Visibility.public(),
  inReplyToId: null,
  repliesCount: 0,
  reblogsCount: 0,
  favouritesCount: 0,
  favourited: false,
  reblogged: false,
  bookmarked: false,
  account: {
    id: "1",
    username: "alice",
    acct: "alice",
    displayName: "Alice",
    avatar: "https://example.test/a.png",
  },
  mediaAttachments: [],
  reblog: null,
});

describe("Status.prependUnique", () => {
  it("prepends a status that is not already in the list", () => {
    const existing = status("s1");
    const incoming = status("s2");
    expect(StatusModel.prependUnique([existing], incoming)).toEqual([incoming, existing]);
  });

  it("does not duplicate when the same id is already present", () => {
    const existing = status("s1");
    expect(StatusModel.prependUnique([existing], status("s1"))).toEqual([existing]);
  });
});
