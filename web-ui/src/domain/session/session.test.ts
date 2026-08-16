import { describe, expect, it } from "vitest";
import { SessionState } from "@/domain/session/session";

const account = {
  id: "1",
  username: "alice",
  displayName: "Alice",
  acct: "alice@example.com",
  avatar: "https://example.com/avatar.png",
  instanceName: "example",
} as const;

describe("SessionState", () => {
  it("detects authenticated sessions", () => {
    const state = SessionState.authenticated(account);
    expect(SessionState.isAuthenticated(state)).toBe(true);
    expect(SessionState.isAuthenticated(SessionState.anonymous())).toBe(false);
  });

  it("resolves loading into an anonymous, authenticated, or failed session", () => {
    const loading = SessionState.loading();
    expect(SessionState.resolve(loading, SessionState.anonymous()).kind).toBe("Anonymous");
    expect(SessionState.resolve(loading, SessionState.authenticated(account)).kind).toBe(
      "Authenticated",
    );
    expect(SessionState.resolve(loading, SessionState.failed("nope")).kind).toBe("Failed");
  });

  it("logs out from an authenticated session", () => {
    expect(SessionState.logout(SessionState.authenticated(account))).toEqual(
      SessionState.anonymous(),
    );
  });

  it("updates the authenticated account in place", () => {
    const session = SessionState.authenticated(account);
    const next = SessionState.updateAccount(session, { ...account, displayName: "Alicia" });
    expect(next.kind).toBe("Authenticated");
    expect(next.account.displayName).toBe("Alicia");
  });
});
