import type { AccountProfile } from "@/domain/account/account";
import type { Status } from "@/domain/status/status";
import { CachedView } from "./cached-view";

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

  lookup: (set: ProfileSet, accountId: string): CachedView<ProfileSnapshot> => {
    const snapshot = set.get(accountId);
    return snapshot === undefined ? CachedView.absent() : CachedView.present(snapshot);
  },

  insert: (set: ProfileSet, accountId: string, snapshot: ProfileSnapshot): ProfileSet => {
    const next = new Map(set);
    next.delete(accountId);
    next.set(accountId, snapshot);
    while (next.size > VIEW_CACHE_MAX_PROFILES) {
      const oldest = next.keys().next().value;
      if (oldest === undefined) {
        break;
      }
      next.delete(oldest);
    }
    return next;
  },

  map: (
    set: ProfileSet,
    update: (snapshot: ProfileSnapshot) => ProfileSnapshot,
  ): ProfileSet => {
    let changed = false;
    const next = new Map<string, ProfileSnapshot>();
    for (const [accountId, snapshot] of set) {
      const updated = update(snapshot);
      if (!Object.is(updated, snapshot)) {
        changed = true;
      }
      next.set(accountId, updated);
    }
    return changed ? next : set;
  },
} as const;
