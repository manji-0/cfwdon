import { describe, expect, it } from "vitest";
import { countUnreadConversations } from "@/domain/conversations/unread";

describe("countUnreadConversations", () => {
  it("sums unread flags", () => {
    expect(countUnreadConversations([{ unread: true }, { unread: true }, { unread: false }])).toBe(
      2,
    );
  });
});
