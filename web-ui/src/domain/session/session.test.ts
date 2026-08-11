import { describe, expect, it } from "vitest";
import { SessionState } from "@/domain/session/session";

describe("SessionState", () => {
  it("detects authenticated sessions", () => {
    const state = SessionState.authenticated({
      id: "1",
      username: "alice",
      displayName: "Alice",
      acct: "alice@example.com",
      avatar: "https://example.com/avatar.png",
      instanceName: "example",
    });
    expect(SessionState.isAuthenticated(state)).toBe(true);
    expect(SessionState.isAuthenticated(SessionState.anonymous())).toBe(false);
  });
});
