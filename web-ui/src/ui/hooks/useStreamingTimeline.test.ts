import { describe, expect, it } from "vitest";
import { Status } from "@/domain/status/status";
import { Visibility } from "@/domain/status/visibility";
import { applyStreamingTimelineEvent } from "@/ui/hooks/useStreamingTimeline";

const status = (id: string) =>
  Status.original({
    id,
    createdAt: "2026-08-13T00:00:00.000Z",
    content: `<p>${id}</p>`,
    spoilerText: "",
    sensitive: false,
    visibility: Visibility.public(),
    inReplyToId: null,
    repliesCount: 0,
    reblogsCount: 0,
    favouritesCount: 0,
    favourited: false,
    reblogged: false,
    bookmarked: false,
    account: {
      id: "1",
      username: "alice",
      acct: "alice",
      displayName: "Alice",
      avatar: "https://example.test/a.png",
    },
    mediaAttachments: [],
    card: null,
  });

describe("applyStreamingTimelineEvent", () => {
  it("does not insert an update that is already in the timeline", () => {
    const existing = status("s1");
    const next = applyStreamingTimelineEvent([existing], {
      kind: "Update",
      status: status("s1"),
    });
    expect(next).toEqual([existing]);
  });

  it("prepends a new update", () => {
    const existing = status("s1");
    const incoming = status("s2");
    expect(
      applyStreamingTimelineEvent([existing], { kind: "Update", status: incoming }),
    ).toEqual([incoming, existing]);
  });
});
