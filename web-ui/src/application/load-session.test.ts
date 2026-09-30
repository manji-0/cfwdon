import { err, ok } from "neverthrow";
import { describe, expect, it } from "vitest";
import { loadSession } from "@/application/load-session";
import type { AccountSummary } from "@/domain/session/account";

const account: AccountSummary = {
  id: "1",
  username: "alice",
  acct: "alice",
  displayName: "Alice",
  avatar: "https://example.test/a.png",
  instanceName: "example.test",
};

describe("loadSession", () => {
  it("resolves authenticated and anonymous sessions from the port", async () => {
    const authed = await loadSession(async () => ok(account));
    expect(authed._unsafeUnwrap()).toEqual({ kind: "Authenticated", account });
    const anon = await loadSession(async () => ok(null));
    expect(anon._unsafeUnwrap().kind).toBe("Anonymous");
  });

  it("maps fetch errors to a failed session message", async () => {
    const failed = await loadSession(async () =>
      err({ kind: "HttpStatus", status: 500, body: "" } as const),
    );
    expect(failed._unsafeUnwrap()).toEqual({
      kind: "Failed",
      message: "セッションの取得に失敗しました (500)",
    });
    const invalid = await loadSession(async () => err({ kind: "ValidationError" } as const));
    expect(invalid._unsafeUnwrap()).toEqual({
      kind: "Failed",
      message: "サーバー応答の形式が不正です",
    });
  });
});
