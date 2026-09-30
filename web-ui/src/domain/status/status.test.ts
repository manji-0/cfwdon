import { describe, expect, it } from "vitest";
import type { AccountRef } from "@/domain/account/account";
import { Status, type OriginalStatus } from "@/domain/status/status";
import { Visibility } from "@/domain/status/visibility";

const account: AccountRef = {
  id: "1",
  username: "alice",
  acct: "alice",
  displayName: "Alice",
  avatar: "https://example.test/a.png",
};

const booster: AccountRef = { ...account, id: "2", username: "bob", acct: "bob", displayName: "Bob" };

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

const boostOf = (body: OriginalStatus, id = `boost-${body.id}`) =>
  Status.boost({ id, createdAt: "2026-08-13T00:00:02.000Z", account: booster, original: body });

describe("Status.original", () => {
  it("fills optional fields with neutral defaults", () => {
    const status = original("s1");
    expect(status.kind).toBe("Original");
    expect(status.poll).toBeNull();
    expect(status.pinned).toBe(false);
    expect(status.editedAt).toBeNull();
    expect(status.quote).toBeNull();
    expect(status.muted).toBe(false);
  });
});

describe("Status.displayBody / boostedBy", () => {
  it("returns the status itself and no booster for originals", () => {
    const status = original("s1");
    expect(Status.displayBody(status)).toBe(status);
    expect(Status.boostedBy(status)).toBeNull();
  });

  it("unwraps boosts to the boosted original and its booster", () => {
    const body = original("s1");
    const boost = boostOf(body);
    expect(Status.displayBody(boost)).toBe(body);
    expect(Status.boostedBy(boost)).toBe(booster);
  });
});

describe("Status.withBody", () => {
  it("replaces an original outright", () => {
    const next = original("s1", true);
    expect(Status.withBody(original("s1"), next)).toBe(next);
  });

  it("keeps the boost identity when the body is unchanged", () => {
    const body = original("s1");
    const boost = boostOf(body);
    expect(Status.withBody(boost, body)).toBe(boost);
  });

  it("rewraps a boost around a new body", () => {
    const boost = boostOf(original("s1"));
    const next = Status.withBody(boost, original("s1", true));
    expect(next).not.toBe(boost);
    expect(next.id).toBe(boost.id);
    expect(Status.displayBody(next).favourited).toBe(true);
  });
});

describe("Status.containsId / findByBodyId", () => {
  const boost = boostOf(original("s1"), "b1");

  it("matches either the wrapper id or the body id", () => {
    expect(Status.containsId([boost], "b1")).toBe(true);
    expect(Status.containsId([boost], "s1")).toBe(true);
    expect(Status.containsId([boost], "s2")).toBe(false);
  });

  it("finds by body id only", () => {
    expect(Status.findByBodyId([boost], "s1")).toBe(boost);
    expect(Status.findByBodyId([boost], "b1")).toBeUndefined();
  });

  it("skips an incoming boost whose wrapper id is already listed", () => {
    const list = [original("s1")];
    expect(Status.prependUnique(list, boostOf(original("s2"), "s1"))).toBe(list);
  });
});

describe("Status.withPoll", () => {
  const poll = {
    id: "p1",
    expiresAt: "2026-08-14T00:00:00.000Z",
    expired: false,
    multiple: false,
    votesCount: 1,
    votersCount: 1,
    voted: true,
    ownVotes: [0],
    options: [
      { title: "a", votesCount: 1 },
      { title: "b", votesCount: 0 },
    ],
  };

  it("sets the poll on the displayed body, including inside boosts", () => {
    expect(Status.displayBody(Status.withPoll(original("s1"), poll)).poll).toEqual(poll);
    const boosted = Status.withPoll(boostOf(original("s1")), poll);
    expect(boosted.kind).toBe("Boost");
    expect(Status.displayBody(boosted).poll).toEqual(poll);
  });
});

describe("Status.removeById", () => {
  it("removes by wrapper id or body id", () => {
    const boost = boostOf(original("s1"), "b1");
    const other = original("s2");
    expect(Status.removeById([boost, other], "b1")).toEqual([other]);
    expect(Status.removeById([boost, other], "s1")).toEqual([other]);
  });

  it("returns the same array when nothing matches", () => {
    const list = [original("s1")];
    expect(Status.removeById(list, "missing")).toBe(list);
  });
});
