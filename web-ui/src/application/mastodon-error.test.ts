import { describe, expect, it } from "vitest";
import { mastodonErrorMessage } from "@/application/mastodon-error";

describe("mastodonErrorMessage", () => {
  it("reads the error field of a Mastodon JSON error body", () => {
    expect(
      mastodonErrorMessage({
        kind: "HttpStatus",
        status: 422,
        body: '{"error":"media attachment 1 is already attached"}',
      }),
    ).toBe("media attachment 1 is already attached");
  });

  it("keeps plain-text bodies and falls back on empty ones", () => {
    expect(mastodonErrorMessage({ kind: "HttpStatus", status: 500, body: "boom" })).toBe("boom");
    expect(mastodonErrorMessage({ kind: "HttpStatus", status: 502, body: " " })).toBe(
      "リクエストに失敗しました (502)",
    );
  });
});
