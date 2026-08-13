import type { AccountProfile } from "@/domain/account/account";
import type { Notification } from "@/domain/notification/notification";
import type { Status } from "@/domain/status/status";
import { Status as StatusModel } from "@/domain/status/status";

/** Skip a duplicate fetch on Strict Mode / instant remount. Streaming views still revalidate after this. */
export const VIEW_CACHE_REMOUNT_SKIP_MS = 2_000;
/** Profile pages have no live stream; skip refetch inside this window. */
export const VIEW_CACHE_PROFILE_FRESH_MS = 30_000;
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

export type ViewCacheStreamEvent =
  | { readonly kind: "update"; readonly status: Status }
  | { readonly kind: "delete"; readonly statusId: string }
  | { readonly kind: "notification"; readonly notification: Notification };

export const ViewCache = {
  empty: (): ViewCacheState => ({
    home: null,
    notifications: null,
    profiles: new Map(),
  }),

  isRemountSkip: (fetchedAt: number, now = Date.now()): boolean =>
    now - fetchedAt < VIEW_CACHE_REMOUNT_SKIP_MS,

  isProfileFresh: (fetchedAt: number, now = Date.now()): boolean =>
    now - fetchedAt < VIEW_CACHE_PROFILE_FRESH_MS,

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

  applyStreamEvent: (state: ViewCacheState, event: ViewCacheStreamEvent): ViewCacheState => {
    switch (event.kind) {
      case "update":
        if (!state.home) {
          return state;
        }
        return {
          ...state,
          home: {
            ...state.home,
            statuses: StatusModel.prependUnique(state.home.statuses, event.status),
          },
        };
      case "notification":
        if (!state.notifications) {
          return state;
        }
        if (state.notifications.notifications.some((item) => item.id === event.notification.id)) {
          return state;
        }
        return {
          ...state,
          notifications: {
            ...state.notifications,
            notifications: [event.notification, ...state.notifications.notifications],
          },
        };
      case "delete":
        return removeStatus(state, event.statusId);
    }
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

const removeStatus = (state: ViewCacheState, statusId: string): ViewCacheState => {
  const profiles = new Map<string, ProfileSnapshot>();
  for (const [accountId, snapshot] of state.profiles) {
    profiles.set(accountId, {
      ...snapshot,
      statuses: snapshot.statuses.filter(
        (item) => item.id !== statusId && item.reblog?.id !== statusId,
      ),
    });
  }
  return {
    home: state.home
      ? {
          ...state.home,
          statuses: state.home.statuses.filter(
            (item) => item.id !== statusId && item.reblog?.id !== statusId,
          ),
        }
      : null,
    notifications: state.notifications
      ? {
          ...state.notifications,
          notifications: state.notifications.notifications.filter(
            (item) => item.status?.id !== statusId,
          ),
        }
      : null,
    profiles,
  };
};
