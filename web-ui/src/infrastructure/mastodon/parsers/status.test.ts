import { describe, expect, it } from "vitest";
import { Status } from "@/domain/status/status";
import { isArkError } from "@/infrastructure/mastodon/parse";
import { parseStatus } from "@/infrastructure/mastodon/parsers/status";

const sampleCard = {
  url: "https://example.com/article",
  title: "Example Article",
  description: "A short summary.",
  type: "link",
  provider_name: "example.com",
  provider_url: "https://example.com",
  image: "https://example.com/og.png",
  blurhash: null,
};

const basePayload = {
  id: "1",
  created_at: "2026-08-13T00:00:00.000Z",
  content: "<p>https://example.com/article</p>",
  visibility: "public" as const,
  account: {
    id: "10",
    username: "alice",
    acct: "alice",
    display_name: "Alice",
    avatar: "https://example.test/a.png",
  },
};

describe("parseStatus card", () => {
  it("maps preview card fields from the API payload", () => {
    const result = parseStatus({ ...basePayload, card: sampleCard });
    expect(isArkError(result)).toBe(false);
    if (isArkError(result)) {
      return;
    }

    expect(Status.displayBody(result).card).toEqual({
      kind: "Link",
      url: sampleCard.url,
      title: sampleCard.title,
      description: sampleCard.description,
      providerName: sampleCard.provider_name,
      providerUrl: sampleCard.provider_url,
      image: sampleCard.image,
      blurhash: null,
    });
  });

  it("defaults card to null when absent", () => {
    const result = parseStatus(basePayload);
    expect(isArkError(result)).toBe(false);
    if (isArkError(result)) {
      return;
    }

    expect(Status.displayBody(result).card).toBeNull();
  });
});

describe("parseStatus boost", () => {
  it("wraps a reblog as Boost around Original", () => {
    const result = parseStatus({
      ...basePayload,
      id: "boost-1",
      account: {
        ...basePayload.account,
        id: "20",
        username: "bob",
        acct: "bob",
        display_name: "Bob",
      },
      reblog: { ...basePayload, id: "orig-1" },
    });
    expect(isArkError(result)).toBe(false);
    if (isArkError(result)) {
      return;
    }
    expect(result.kind).toBe("Boost");
    if (result.kind !== "Boost") {
      return;
    }
    expect(result.id).toBe("boost-1");
    expect(result.account.username).toBe("bob");
    expect(result.original.kind).toBe("Original");
    expect(result.original.id).toBe("orig-1");
  });
});

describe("parseStatus poll", () => {
  it("maps poll fields from the API payload", () => {
    const result = parseStatus({
      ...basePayload,
      poll: {
        id: "poll-1",
        expires_at: "2026-08-23T00:00:00.000Z",
        expired: false,
        multiple: false,
        votes_count: 4,
        voters_count: 4,
        voted: false,
        own_votes: [],
        options: [
          { title: "yes", votes_count: 1 },
          { title: "no", votes_count: 3 },
        ],
      },
    });
    expect(isArkError(result)).toBe(false);
    if (isArkError(result)) {
      return;
    }
    expect(Status.displayBody(result).poll).toEqual({
      id: "poll-1",
      expiresAt: "2026-08-23T00:00:00.000Z",
      expired: false,
      multiple: false,
      votesCount: 4,
      votersCount: 4,
      voted: false,
      ownVotes: [],
      options: [
        { title: "yes", votesCount: 1 },
        { title: "no", votesCount: 3 },
      ],
    });
  });
});

describe("parseStatus high-priority fields", () => {
  it("maps pin, edit timestamp, and nested quotes", () => {
    const result = parseStatus({
      ...basePayload,
      pinned: true,
      edited_at: "2026-08-22T01:00:00.000Z",
      quote: {
        state: "accepted",
        quoted_status: {
          ...basePayload,
          id: "quoted-1",
          content: "<p>quoted</p>",
        },
      },
    });
    expect(isArkError(result)).toBe(false);
    if (isArkError(result)) {
      return;
    }
    const body = Status.displayBody(result);
    expect(body.pinned).toBe(true);
    expect(body.editedAt).toBe("2026-08-22T01:00:00.000Z");
    expect(body.quote).toEqual({
      state: "accepted",
      quotedStatus: {
        id: "quoted-1",
        content: "<p>quoted</p>",
        spoilerText: "",
        account: {
          id: "10",
          username: "alice",
          acct: "alice",
          displayName: "Alice",
          avatar: "https://example.test/a.png",
        },
      },
    });
  });
});
