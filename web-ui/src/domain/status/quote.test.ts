import { describe, expect, it } from "vitest";
import { StatusQuote } from "@/domain/status/quote";

describe("StatusQuote", () => {
  it("is visible only when accepted with a quoted status", () => {
    expect(
      StatusQuote.isVisible({
        state: "accepted",
        quotedStatus: {
          id: "1",
          content: "<p>hi</p>",
          spoilerText: "",
          account: {
            id: "a",
            username: "alice",
            acct: "alice",
            displayName: "Alice",
            avatar: "",
          },
        },
      }),
    ).toBe(true);
    expect(StatusQuote.isVisible({ state: "pending", quotedStatus: null })).toBe(false);
  });
});
