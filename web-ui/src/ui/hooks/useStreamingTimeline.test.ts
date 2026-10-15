import { describe, expect, it } from "vitest";
import { Status } from "@/domain/status/status";
import { Visibility } from "@/domain/status/visibility";
import { applyStreamingTimelineEvent } from "@/ui/hooks/useStreamingTimeline";

const status = (id: string, createdAt = "2026-08-13T00:00:00.000Z") =>
  Status.original({
    id,
    createdAt,
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

  it("inserts a backfilled update below newer statuses", () => {
    const newer = status("s1", "2026-08-13T02:00:00.000Z");
    const older = status("s2", "2026-08-13T01:00:00.000Z");

    const next = applyStreamingTimelineEvent([newer], { kind: "Update", status: older });

    expect(next.map((item) => item.id)).toEqual(["s1", "s2"]);
  });

  it("keeps newest-first order between backfilled updates", () => {
    const newest = status("s1", "2026-08-13T02:00:00.000Z");
    const middle = status("s2", "2026-08-13T01:00:00.000Z");
    const oldest = status("s3", "2026-08-13T00:00:00.000Z");

    const next = applyStreamingTimelineEvent([newest, middle], {
      kind: "Update",
      status: oldest,
    });

    expect(next.map((item) => item.id)).toEqual(["s1", "s2", "s3"]);
  });
});
