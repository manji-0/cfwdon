import { describe, expect, it } from "vitest";
import type { AccountProfile } from "@/domain/account/account";
import type { AccountRef } from "@/domain/account/account";
import { CachedView } from "@/domain/cache/cached-view";
import { ProfileSet, VIEW_CACHE_MAX_PROFILES } from "@/domain/cache/profile-set";
import { SelfProfilePreload } from "@/domain/cache/self-profile-preload";
import {
  ViewCache,
  type NotificationsSnapshot,
  type ProfileSnapshot,
  type TimelineSnapshot,
} from "@/domain/cache/view-cache";
import {
  VIEW_CACHE_PROFILE_FRESH_MS,
  VIEW_CACHE_REMOUNT_SKIP_MS,
  ViewReadiness,
} from "@/domain/cache/view-readiness";
import type { Notification } from "@/domain/notification/notification";
import { SessionState } from "@/domain/session/session";
import type { Status } from "@/domain/status/status";
import { Visibility } from "@/domain/status/visibility";

const account = {
  id: "1",
  username: "alice",
  acct: "alice",
  displayName: "Alice",
  avatar: "https://example.test/a.png",
} as const satisfies AccountRef;

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

const homeSnapshot = (statuses: ReadonlyArray<Status>, fetchedAt = 1): TimelineSnapshot => ({
  statuses,
  fetchedAt,
  scrollY: 0,
});

const notificationsSnapshot = (
  notifications: ReadonlyArray<Notification>,
  fetchedAt = 1,
): NotificationsSnapshot => ({
  notifications,
  fetchedAt,
  scrollY: 0,
});

const profileSnapshot = (id: string, fetchedAt = 1): ProfileSnapshot => ({
  profile: profile(id),
  statuses: [],
  fetchedAt,
  scrollY: 0,
});

const mention = {
  id: "n1",
  type: "mention",
  groupKey: "n1",
  createdAt: "2026-08-13T00:00:00.000Z",
  account,
  status: status("s2"),
} as const satisfies Notification;

describe("ViewReadiness", () => {
  it("loads streaming views that are still absent", () => {
    expect(ViewReadiness.forStreaming(CachedView.absent(), 1_000_000)).toEqual(
      ViewReadiness.load(),
    );
  });

  it("skips a streaming remount only inside the remount window", () => {
    const now = 1_000_000;
    const fresh = CachedView.present(homeSnapshot([status("s1")], now - VIEW_CACHE_REMOUNT_SKIP_MS + 1));
    const stale = CachedView.present(homeSnapshot([status("s1")], now - VIEW_CACHE_REMOUNT_SKIP_MS));
    expect(ViewReadiness.forStreaming(fresh, now)).toEqual(ViewReadiness.skip());
    expect(ViewReadiness.forStreaming(stale, now)).toEqual(ViewReadiness.revalidate());
  });

  it("skips profile views only inside the freshness window", () => {
    const now = 1_000_000;
    const fresh = CachedView.present(profileSnapshot("1", now - VIEW_CACHE_PROFILE_FRESH_MS + 1));
    const stale = CachedView.present(profileSnapshot("1", now - VIEW_CACHE_PROFILE_FRESH_MS));
    expect(ViewReadiness.forProfile(CachedView.absent(), now)).toEqual(ViewReadiness.load());
    expect(ViewReadiness.forProfile(fresh, now)).toEqual(ViewReadiness.skip());
    expect(ViewReadiness.forProfile(stale, now)).toEqual(ViewReadiness.revalidate());
  });
});

describe("ProfileSet", () => {
  it("evicts the oldest member when insert exceeds capacity", () => {
    const set = Array.from({ length: VIEW_CACHE_MAX_PROFILES + 1 }, (_, index) => String(index)).reduce(
      (current, id) => ProfileSet.insert(current, id, profileSnapshot(id, Number(id))),
      ProfileSet.empty(),
    );
    expect(set.size).toBe(VIEW_CACHE_MAX_PROFILES);
    expect(ProfileSet.has(set, "0")).toBe(false);
    expect(ProfileSet.has(set, String(VIEW_CACHE_MAX_PROFILES))).toBe(true);
  });
});

describe("SelfProfilePreload", () => {
  const session = SessionState.authenticated({
    ...account,
    instanceName: "example",
  });

  it("skips when the session is not authenticated", () => {
    expect(SelfProfilePreload.decide(SessionState.anonymous(), CachedView.absent())).toEqual(
      SelfProfilePreload.skip("NotAuthenticated"),
    );
  });

  it("skips when the self profile is already present", () => {
    expect(SelfProfilePreload.decide(session, CachedView.present(profileSnapshot("1")))).toEqual(
      SelfProfilePreload.skip("AlreadyCached"),
    );
  });

  it("fetches when authenticated and the self profile is absent", () => {
    expect(SelfProfilePreload.decide(session, CachedView.absent())).toEqual(
      SelfProfilePreload.fetch("1"),
    );
  });
});

describe("ViewCache", () => {
  it("does not overwrite a present profile during preload", () => {
    const opened = profileSnapshot("1", 10);
    const preloaded = { ...profileSnapshot("1", 99), scrollY: 40 };
    const state = ViewCache.writeProfile(ViewCache.empty(), "1", opened);
    const next = ViewCache.receivePreloadedProfile(state, "1", preloaded);
    expect(ProfileSet.lookup(next.profiles, "1")).toEqual(CachedView.present(opened));
  });

  it("inserts a preloaded profile only from Absent", () => {
    const snapshot = profileSnapshot("1");
    const next = ViewCache.receivePreloadedProfile(ViewCache.empty(), "1", snapshot);
    expect(ProfileSet.lookup(next.profiles, "1")).toEqual(CachedView.present(snapshot));
  });

  it("patches a status in the home timeline and profile timelines", () => {
    const original = status("s1");
    let state = ViewCache.writeHome(ViewCache.empty(), homeSnapshot([original]));
    state = ViewCache.writeProfile(state, "1", {
      ...profileSnapshot("1"),
      statuses: [original],
    });
    const updated = status("s1", true);
    state = ViewCache.patchStatus(state, updated);
    expect(state.home).toEqual(
      CachedView.present({
        ...homeSnapshot([updated]),
      }),
    );
    const cachedProfile = ProfileSet.lookup(state.profiles, "1");
    expect(cachedProfile.kind === "Present" && cachedProfile.value.statuses[0]?.favourited).toBe(true);
  });

  it("applies live stream events only to present snapshots", () => {
    let state = ViewCache.writeHome(ViewCache.empty(), homeSnapshot([status("s1")]));
    state = ViewCache.writeNotifications(state, notificationsSnapshot([]));

    state = ViewCache.applyStreamEvent(state, { kind: "update", status: status("s2") });
    expect(state.home).toEqual(CachedView.present(homeSnapshot([status("s2"), status("s1")])));

    state = ViewCache.applyStreamEvent(state, { kind: "notification", notification: mention });
    expect(state.notifications).toEqual(CachedView.present(notificationsSnapshot([mention])));

    state = ViewCache.applyStreamEvent(state, { kind: "delete", statusId: "s2" });
    expect(state.home).toEqual(CachedView.present(homeSnapshot([status("s1")])));
    expect(state.notifications).toEqual(CachedView.present(notificationsSnapshot([])));
  });

  it("does not invent snapshots from stream events before the first fetch", () => {
    const next = ViewCache.applyStreamEvent(ViewCache.empty(), {
      kind: "update",
      status: status("s1"),
    });
    expect(next.home).toEqual(CachedView.absent());
  });
});
