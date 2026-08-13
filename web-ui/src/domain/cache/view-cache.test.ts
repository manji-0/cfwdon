import { describe, expect, it } from "vitest";
import type { AccountProfile } from "@/domain/account/account";
import { ViewCache, VIEW_CACHE_FRESH_MS, VIEW_CACHE_MAX_PROFILES } from "@/domain/cache/view-cache";
import type { Status } from "@/domain/status/status";
import { Visibility } from "@/domain/status/visibility";

const account = {
  id: "1",
  username: "alice",
  acct: "alice",
  displayName: "Alice",
  avatar: "https://example.test/a.png",
} as const;

const status = (id: string, favourited = false): Status => ({
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
  reblog: null,
});

const profile = (id: string): AccountProfile => ({
  id,
  username: `user${id}`,
  acct: `user${id}`,
  displayName: `User ${id}`,
  avatar: "https://example.test/a.png",
  header: "",
  note: "",
  followersCount: 0,
  followingCount: 0,
  statusesCount: 0,
  locked: false,
});

describe("ViewCache", () => {
  it("treats snapshots newer than the freshness window as fresh", () => {
    const now = 1_000_000;
    expect(ViewCache.isFresh(now - VIEW_CACHE_FRESH_MS + 1, now)).toBe(true);
    expect(ViewCache.isFresh(now - VIEW_CACHE_FRESH_MS, now)).toBe(false);
  });

  it("evicts the oldest profile when over the cap", () => {
    let state = ViewCache.empty();
    for (let index = 0; index < VIEW_CACHE_MAX_PROFILES + 1; index += 1) {
      const id = String(index);
      state = ViewCache.writeProfile(state, id, {
        profile: profile(id),
        statuses: [],
        fetchedAt: index,
        scrollY: 0,
      });
    }
    expect(state.profiles.size).toBe(VIEW_CACHE_MAX_PROFILES);
    expect(state.profiles.has("0")).toBe(false);
    expect(state.profiles.has(String(VIEW_CACHE_MAX_PROFILES))).toBe(true);
  });

  it("patches a status in the home timeline and profile timelines", () => {
    const original = status("s1");
    let state = ViewCache.writeHome(ViewCache.empty(), {
      statuses: [original],
      fetchedAt: 1,
      scrollY: 0,
    });
    state = ViewCache.writeProfile(state, "1", {
      profile: profile("1"),
      statuses: [original],
      fetchedAt: 1,
      scrollY: 0,
    });
    const updated = status("s1", true);
    state = ViewCache.patchStatus(state, updated);
    expect(state.home?.statuses[0]?.favourited).toBe(true);
    expect(state.profiles.get("1")?.statuses[0]?.favourited).toBe(true);
  });
});
