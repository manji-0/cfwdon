import type { AccountProfile } from "@/domain/account/account";
import type { Notification } from "@/domain/notification/notification";
import type { Status } from "@/domain/status/status";
import { Status as StatusModel } from "@/domain/status/status";

export const VIEW_CACHE_FRESH_MS = 30_000;
export const VIEW_CACHE_MAX_PROFILES = 20;

export type TimelineSnapshot = Readonly<{
  statuses: ReadonlyArray<Status>;
  fetchedAt: number;
  scrollY: number;
}>;

export type NotificationsSnapshot = Readonly<{
  notifications: ReadonlyArray<Notification>;
  fetchedAt: number;
  scrollY: number;
}>;

export type ProfileSnapshot = Readonly<{
  profile: AccountProfile;
  statuses: ReadonlyArray<Status>;
  fetchedAt: number;
  scrollY: number;
}>;

export type ViewCacheState = Readonly<{
  home: TimelineSnapshot | null;
  notifications: NotificationsSnapshot | null;
  profiles: ReadonlyMap<string, ProfileSnapshot>;
}>;

export const ViewCache = {
  empty: (): ViewCacheState => ({
    home: null,
    notifications: null,
    profiles: new Map(),
  }),

  isFresh: (fetchedAt: number, now = Date.now()): boolean => now - fetchedAt < VIEW_CACHE_FRESH_MS,

  writeHome: (state: ViewCacheState, snapshot: TimelineSnapshot): ViewCacheState => ({
    ...state,
    home: snapshot,
  }),

  writeNotifications: (state: ViewCacheState, snapshot: NotificationsSnapshot): ViewCacheState => ({
    ...state,
    notifications: snapshot,
  }),

  writeProfile: (
    state: ViewCacheState,
    accountId: string,
    snapshot: ProfileSnapshot,
  ): ViewCacheState => {
    const profiles = new Map(state.profiles);
    profiles.delete(accountId);
    profiles.set(accountId, snapshot);
    while (profiles.size > VIEW_CACHE_MAX_PROFILES) {
      const oldest = profiles.keys().next().value;
      if (oldest === undefined) {
        break;
      }
      profiles.delete(oldest);
    }
    return { ...state, profiles };
  },

  patchStatus: (state: ViewCacheState, updated: Status): ViewCacheState => {
    const profiles = new Map<string, ProfileSnapshot>();
    for (const [accountId, snapshot] of state.profiles) {
      profiles.set(accountId, {
        ...snapshot,
        statuses: StatusModel.replaceInList(snapshot.statuses, updated),
      });
    }
    return {
      home: state.home
        ? { ...state.home, statuses: StatusModel.replaceInList(state.home.statuses, updated) }
        : null,
      notifications: state.notifications
        ? {
            ...state.notifications,
            notifications: state.notifications.notifications.map((notification) => {
              if (!notification.status) {
                return notification;
              }
              const [next] = StatusModel.replaceInList([notification.status], updated);
              return next === notification.status ? notification : { ...notification, status: next };
            }),
          }
        : null,
      profiles,
    };
  },
} as const;
