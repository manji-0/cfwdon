import { describe, expect, it } from "vitest";
import { Conversation } from "@/domain/conversations/conversation";
import { ConversationSet } from "@/domain/conversations/conversation-set";
import { countUnreadConversations } from "@/domain/conversations/unread";

const conversation = (id: string, kind: "Read" | "Unread") => {
  const fields = { id, accounts: [], lastStatus: null };
  return kind === "Unread" ? Conversation.unread(fields) : Conversation.read(fields);
};

describe("ConversationSet", () => {
  it("counts unread members", () => {
    const set = ConversationSet.replace([
      conversation("1", "Unread"),
      conversation("2", "Unread"),
      conversation("3", "Read"),
    ]);
    expect(ConversationSet.unreadCount(set)).toBe(2);
    expect(countUnreadConversations(set)).toBe(2);
  });

  it("upserts by id at the front and does not invent members", () => {
    const first = conversation("1", "Read");
    const updated = conversation("1", "Unread");
    const other = conversation("2", "Read");
    const next = ConversationSet.upsert(ConversationSet.replace([first, other]), updated);
    expect(next.map((item) => item.id)).toEqual(["1", "2"]);
    expect(next[0]).toEqual(updated);
  });

  it("marks an unread conversation read", () => {
    const unread = Conversation.unread({ id: "1", accounts: [], lastStatus: null });
    expect(Conversation.markRead(unread)).toEqual(
      Conversation.read({ id: "1", accounts: [], lastStatus: null }),
    );
  });
});
