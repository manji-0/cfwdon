import { describe, expect, it } from "vitest";
import type { AccountRef } from "@/domain/account/account";
import { Notification } from "@/domain/notification/notification";
import { Status } from "@/domain/status/status";
import { Visibility } from "@/domain/status/visibility";

const account = {
  id: "1",
  username: "alice",
  acct: "alice@example.com",
  displayName: "Alice",
  avatar: "https://example.com/a.png",
} as const satisfies AccountRef;

const status = Status.original({
  id: "s1",
  createdAt: "2026-01-01T00:00:00.000Z",
  content: "<p>hi</p>",
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
  account,
  mediaAttachments: [],
  card: null,
});

const meta = {
  id: "n1",
  groupKey: "g1",
  createdAt: "2026-01-01T00:00:00.000Z",
  account,
} as const;

describe("Notification", () => {
  it("labels follow notifications without a status", () => {
    const notification = Notification.follow(meta);
    expect(Notification.label(notification)).toBe("Alice がフォローしました");
    expect(Notification.status(notification)).toBeNull();
  });

  it("requires a status on mention notifications", () => {
    const notification = Notification.mention({ ...meta, status });
    expect(Notification.label(notification)).toBe("Alice が返信しました");
    expect(Notification.status(notification)).toBe(status);
  });
});
