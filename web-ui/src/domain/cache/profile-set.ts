import type { AccountProfile } from "@/domain/account/account";
import type { Status } from "@/domain/status/status";
import { CachedView, type CachedView as CachedSlot } from "./cached-view";

export const VIEW_CACHE_MAX_PROFILES = 20;

export type ProfileSnapshot = Readonly<{
  profile: AccountProfile;
  statuses: ReadonlyArray<Status>;
  fetchedAt: number;
  scrollY: number;
}>;

export type ProfileSet = ReadonlyMap<string, ProfileSnapshot>;

export const ProfileSet = {
  empty: (): ProfileSet => new Map(),

  has: (set: ProfileSet, accountId: string) => set.has(accountId),

  lookup: (set: ProfileSet, accountId: string): CachedSlot<ProfileSnapshot> => {
    const snapshot = set.get(accountId);
    return snapshot === undefined ? CachedView.absent() : CachedView.present(snapshot);
  },

  insert: (set: ProfileSet, accountId: string, snapshot: ProfileSnapshot): ProfileSet => {
    const next = [...set].filter(([id]) => id !== accountId);
    const entries: ReadonlyArray<readonly [string, ProfileSnapshot]> = [
      ...next,
      [accountId, snapshot],
    ];
    const kept =
      entries.length > VIEW_CACHE_MAX_PROFILES
        ? entries.slice(entries.length - VIEW_CACHE_MAX_PROFILES)
        : entries;
    return new Map(kept);
  },

  map: (
    set: ProfileSet,
    update: (snapshot: ProfileSnapshot) => ProfileSnapshot,
  ): ProfileSet => new Map([...set].map(([accountId, snapshot]) => [accountId, update(snapshot)])),
} as const;
