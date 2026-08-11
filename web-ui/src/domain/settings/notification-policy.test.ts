import { describe, expect, it } from "vitest";
import { NotificationPolicy } from "@/domain/settings/notification-policy";

describe("NotificationPolicy", () => {
  it("parses worker notification policy payloads", () => {
    const parsed = NotificationPolicy.schema.parse({
      for_not_following: "accept",
      for_not_followers: "filter",
      for_new_accounts: "accept",
      for_private_mentions: "drop",
      for_limited_accounts: "filter",
    });

    expect(parsed.forNotFollowing).toBe("accept");
    expect(parsed.forPrivateMentions).toBe("drop");
  });
});
