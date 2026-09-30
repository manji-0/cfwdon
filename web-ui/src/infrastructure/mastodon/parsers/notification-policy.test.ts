import { describe, expect, it } from "vitest";
import { parseNotificationPolicy } from "@/infrastructure/mastodon/parsers/notification-policy";
import { isArkError } from "@/infrastructure/mastodon/parse";

describe("parseNotificationPolicy", () => {
  it("parses worker notification policy payloads", () => {
    const result = parseNotificationPolicy({
      for_not_following: "accept",
      for_not_followers: "filter",
      for_new_accounts: "accept",
      for_private_mentions: "drop",
      for_limited_accounts: "filter",
    });

    if (isArkError(result)) {
      throw new Error(result.summary);
    }

    expect(result.forNotFollowing).toBe("accept");
    expect(result.forPrivateMentions).toBe("drop");
  });
});
