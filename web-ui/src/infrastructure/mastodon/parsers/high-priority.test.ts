import { describe, expect, it } from "vitest";
import { isArkError } from "@/infrastructure/mastodon/parse";
import { parseCustomEmojiList } from "@/infrastructure/mastodon/parsers/emoji";
import { parseKeywordFilter } from "@/infrastructure/mastodon/parsers/filter";
import {
  parseStatusSource,
  parseStatusTranslation,
} from "@/infrastructure/mastodon/parsers/status-extra";
import { parseFollowedTag } from "@/infrastructure/mastodon/parsers/tag";
import { parseTrendLinkList } from "@/infrastructure/mastodon/parsers/trend-link";
import { parseAccountProfile } from "@/infrastructure/mastodon/parsers/account";
import { parseScheduledStatus } from "@/infrastructure/mastodon/parsers/scheduled";
import { parseAnnouncement } from "@/infrastructure/mastodon/parsers/announcement";
import { parseFeaturedTag } from "@/infrastructure/mastodon/parsers/featured-tag";

describe("high-priority Mastodon parsers", () => {
  it("parses status source and translation", () => {
    const source = parseStatusSource({
      id: "1",
      text: "hello",
      spoiler_text: "cw",
    });
    expect(isArkError(source)).toBe(false);
    if (!isArkError(source)) {
      expect(source).toEqual({ id: "1", text: "hello", spoilerText: "cw" });
    }

    const translation = parseStatusTranslation({
      content: "<p>こんにちは</p>",
      spoiler_text: "",
      detected_source_language: "en",
      language: "ja",
      provider: "cfwdon",
    });
    expect(isArkError(translation)).toBe(false);
    if (!isArkError(translation)) {
      expect(translation.content).toBe("<p>こんにちは</p>");
      expect(translation.provider).toBe("cfwdon");
    }
  });

  it("parses v2 keyword filters", () => {
    const result = parseKeywordFilter({
      id: "filter-1",
      title: "spam",
      context: ["home", "notifications"],
      expires_at: null,
      filter_action: "warn",
      keywords: [{ id: "kw-1", keyword: "ads", whole_word: false }],
      statuses: [],
    });
    expect(isArkError(result)).toBe(false);
    if (isArkError(result)) {
      return;
    }
    expect(result.title).toBe("spam");
    expect(result.keywords).toEqual([{ id: "kw-1", keyword: "ads", wholeWord: false }]);
  });

  it("parses custom emojis, followed tags, and trend links", () => {
    const emojis = parseCustomEmojiList([
      {
        shortcode: "blobcat",
        url: "https://example.test/blobcat.png",
        static_url: "https://example.test/blobcat.png",
        visible_in_picker: true,
        category: "cats",
      },
    ]);
    expect(isArkError(emojis)).toBe(false);
    if (!isArkError(emojis)) {
      expect(emojis[0]?.shortcode).toBe("blobcat");
    }

    const tag = parseFollowedTag({
      id: "fediverse",
      name: "fediverse",
      url: "https://example.test/tags/fediverse",
      following: true,
      history: [{ day: "2026-08-22", uses: "3", accounts: "2" }],
    });
    expect(isArkError(tag)).toBe(false);
    if (!isArkError(tag)) {
      expect(tag.following).toBe(true);
    }

    const links = parseTrendLinkList([
      {
        url: "https://example.com/story",
        title: "Story",
        description: "A link",
        image: null,
      },
    ]);
    expect(isArkError(links)).toBe(false);
    if (!isArkError(links)) {
      expect(links[0]?.title).toBe("Story");
    }
  });
});

describe("profile and scheduled parsers", () => {
  it("maps profile fields, bot, and discoverable", () => {
    const result = parseAccountProfile({
      id: "1",
      username: "alice",
      acct: "alice",
      display_name: "Alice",
      avatar: "https://example.test/a.png",
      header: "",
      note: "<p>hi</p>",
      followers_count: 1,
      following_count: 2,
      statuses_count: 3,
      locked: true,
      bot: true,
      discoverable: false,
      fields: [{ name: "site", value: "<a href=\"https://example.test\">ex</a>", verified_at: null }],
    });
    expect(isArkError(result)).toBe(false);
    if (isArkError(result)) {
      return;
    }
    expect(result.locked).toBe(true);
    expect(result.bot).toBe(true);
    expect(result.discoverable).toBe(false);
    expect(result.fields[0]?.name).toBe("site");
  });

  it("parses scheduled status params", () => {
    const result = parseScheduledStatus({
      id: "sched-1",
      scheduled_at: "2026-09-07T12:00:00.000Z",
      params: {
        text: "later",
        spoiler_text: null,
        visibility: "unlisted",
        sensitive: false,
      },
    });
    expect(isArkError(result)).toBe(false);
    if (isArkError(result)) {
      return;
    }
    expect(result.text).toBe("later");
    expect(result.visibility).toBe("unlisted");
  });
});

describe("announcement and featured tag parsers", () => {
  it("parses announcements with read state", () => {
    const result = parseAnnouncement({
      id: "ann-1",
      content: "<p>Hello</p>",
      read: false,
      mentions: [],
      statuses: [],
      tags: [],
      emojis: [],
      reactions: [],
    });
    expect(isArkError(result)).toBe(false);
    if (isArkError(result)) {
      return;
    }
    expect(result.id).toBe("ann-1");
    expect(result.content).toBe("<p>Hello</p>");
    expect(result.read).toBe(false);
  });

  it("parses featured tags", () => {
    const result = parseFeaturedTag({
      id: "cfwdon",
      name: "cfwdon",
      url: "https://example.test/@alice/tagged/cfwdon",
      statuses_count: 4,
      last_status_at: "2026-09-01T00:00:00.000Z",
    });
    expect(isArkError(result)).toBe(false);
    if (isArkError(result)) {
      return;
    }
    expect(result.name).toBe("cfwdon");
    expect(result.statusesCount).toBe(4);
  });
});
