import { describe, expect, it } from "vitest";
import type { AccountRef } from "@/domain/account/account";
import { Status } from "@/domain/status/status";
import { Visibility } from "@/domain/status/visibility";

const account: AccountRef = {
  id: "1",
  username: "alice",
  acct: "alice",
  displayName: "Alice",
  avatar: "https://example.test/a.png",
};

const original = (id: string, favourited = false) =>
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
    favourited,
    reblogged: false,
    bookmarked: false,
    account,
    mediaAttachments: [],
    card: null,
  });

describe("Status.prependUnique", () => {
  it("prepends a status that is not already in the list", () => {
    const existing = original("s1");
    const incoming = original("s2");
    expect(Status.prependUnique([existing], incoming)).toEqual([incoming, existing]);
  });

  it("returns the same array when the id is already present", () => {
    const existing = original("s1");
    const list = [existing];
    expect(Status.prependUnique(list, original("s1"))).toBe(list);
  });
});

describe("Status.appendUnique", () => {
  it("appends statuses that are not already in the list", () => {
    const existing = original("s1");
    const incoming = original("s2");
    expect(Status.appendUnique([existing], [incoming, original("s1")])).toEqual([
      existing,
      incoming,
    ]);
  });

  it("returns the same array when every id is already present", () => {
    const existing = original("s1");
    const list = [existing];
    expect(Status.appendUnique(list, [original("s1")])).toBe(list);
  });
});

describe("Status.replaceInList", () => {
  it("replaces an original and keeps the array when nothing matches", () => {
    const existing = original("s1");
    const list = [existing];
    expect(Status.replaceInList(list, original("missing", true))).toBe(list);
    expect(Status.displayBody(Status.replaceInList(list, original("s1", true))[0]!).favourited).toBe(
      true,
    );
  });

  it("updates the original inside a boost wrapper", () => {
    const body = original("s1");
    const boost = Status.boost({
      id: "boost-1",
      createdAt: "2026-08-13T00:00:01.000Z",
      account,
      original: body,
    });
    const [next] = Status.replaceInList([boost], original("s1", true));
    expect(next?.kind).toBe("Boost");
    if (next?.kind !== "Boost") {
      return;
    }
    expect(next.id).toBe("boost-1");
    expect(next.original.favourited).toBe(true);
    expect(Status.displayBody(next).id).toBe("s1");
    expect(Status.boostedBy(next)).toEqual(account);
  });
});

describe("Status.visibleCard", () => {
  const card = {
    kind: "Link" as const,
    url: "https://example.com",
    title: "Example",
    description: "",
    providerName: "example.com",
    providerUrl: "https://example.com",
    image: null,
    blurhash: null,
  };

  it("returns the card when there is no media", () => {
    expect(Status.visibleCard(original("s1"))).toBeNull();
    const withCard = Status.original({ ...original("s1"), card });
    expect(Status.visibleCard(withCard)).toEqual(card);
  });

  it("hides the card when media attachments are present", () => {
    const withMedia = Status.original({
      ...original("s1"),
      card,
      mediaAttachments: [
        {
          kind: "Image",
          id: "m1",
          url: "https://example.test/image.png",
          previewUrl: "https://example.test/image.png",
          description: null,
        },
      ],
    });
    expect(Status.visibleCard(withMedia)).toBeNull();
  });
});
