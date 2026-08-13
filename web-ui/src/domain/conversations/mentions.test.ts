import { describe, expect, it } from "vitest";
import { conversationAcctsLabel, conversationTitle } from "@/domain/conversations/participants";
import { ensureDirectMentions } from "@/domain/conversations/mentions";

const alice = {
  id: "1",
  username: "alice",
  acct: "alice",
  displayName: "Alice",
  avatar: "https://example.test/a.png",
};

const bob = {
  ...alice,
  id: "2",
  username: "bob",
  acct: "bob@remote.test",
  displayName: "Bob",
};

describe("conversationTitle", () => {
  it("joins display names", () => {
    expect(conversationTitle([])).toBe("会話");
    expect(conversationTitle([alice, bob])).toBe("Alice、Bob");
  });
});

describe("conversationAcctsLabel", () => {
  it("joins @acct handles", () => {
    expect(conversationAcctsLabel([alice, bob])).toBe("@alice @bob@remote.test");
  });
});

describe("ensureDirectMentions", () => {
  it("prefixes missing mentions", () => {
    expect(ensureDirectMentions("hello", [alice, bob])).toBe("@alice @bob@remote.test hello");
  });

  it("leaves already-mentioned participants alone", () => {
    expect(ensureDirectMentions("@alice hello", [alice])).toBe("@alice hello");
  });
});
