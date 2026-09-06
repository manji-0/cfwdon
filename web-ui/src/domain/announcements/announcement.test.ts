import { describe, expect, it } from "vitest";
import { Announcement } from "@/domain/announcements/announcement";

const announcement = (read: boolean): Announcement =>
  ({
    id: "ann-1",
    content: "<p>Hello</p>",
    read,
    publishedAt: null,
  }) as const satisfies Announcement;

describe("Announcement", () => {
  it("treats unread items as those that are not marked read", () => {
    expect(Announcement.isUnread(announcement(false))).toBe(true);
    expect(Announcement.isUnread(announcement(true))).toBe(false);
  });
});
