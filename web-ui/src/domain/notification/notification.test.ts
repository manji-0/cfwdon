import { describe, expect, it } from "vitest";
import { NotificationModel } from "@/domain/notification/notification";
import { parseAccountRef } from "@/infrastructure/mastodon/parsers/account";
import { isArkError } from "@/infrastructure/mastodon/parse";

const accountResult = parseAccountRef({
  id: "1",
  username: "alice",
  acct: "alice@example.com",
  display_name: "Alice",
  avatar: "https://example.com/a.png",
});

if (isArkError(accountResult)) {
  throw new Error(accountResult.summary);
}

const account = accountResult;

describe("NotificationModel", () => {
  it("labels follow notifications", () => {
    expect(
      NotificationModel.label({
        id: "n1",
        type: "follow",
        groupKey: "g1",
        createdAt: "2026-01-01T00:00:00.000Z",
        account,
        status: null,
      }),
    ).toBe("Alice がフォローしました");
  });
});
