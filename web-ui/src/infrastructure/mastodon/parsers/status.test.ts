import { describe, expect, it } from "vitest";
import { PreviewCard } from "@/domain/status/preview-card";
import type { Status } from "@/domain/status/status";
import { Visibility } from "@/domain/status/visibility";
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

    expect(result.card).toEqual({
      url: sampleCard.url,
      title: sampleCard.title,
      description: sampleCard.description,
      type: sampleCard.type,
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

    expect(result.card).toBeNull();
  });
});

describe("PreviewCard.isVisible", () => {
  const status = (overrides: Partial<Status>): Status => ({
    id: "1",
    createdAt: "2026-08-13T00:00:00.000Z",
    content: "<p>https://example.com</p>",
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
      id: "10",
      username: "alice",
      acct: "alice",
      displayName: "Alice",
      avatar: "https://example.test/a.png",
    },
    mediaAttachments: [],
    card: {
      url: "https://example.com",
      title: "Example",
      description: "",
      type: "link",
      providerName: "example.com",
      providerUrl: "https://example.com",
      image: null,
      blurhash: null,
    },
    reblog: null,
    ...overrides,
  });

  it("shows the card when there is no media", () => {
    expect(PreviewCard.isVisible(status({}))).toBe(true);
  });

  it("hides the card when media attachments are present", () => {
    expect(
      PreviewCard.isVisible(
        status({
          mediaAttachments: [
            {
              id: "m1",
              type: "image",
              url: "https://example.test/image.png",
              previewUrl: "https://example.test/image.png",
              description: null,
            },
          ],
        }),
      ),
    ).toBe(false);
  });

  it("hides the card when card is null", () => {
    expect(PreviewCard.isVisible(status({ card: null }))).toBe(false);
  });
});
