import { Notification } from "@/domain/notification/notification";
import { Status } from "@/domain/status/status";
import { CachedView, type PresentView } from "./cached-view";
import { ProfileSet, type ProfileSnapshot } from "./profile-set";

export type { ProfileSnapshot } from "./profile-set";

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

export type ViewCacheState = Readonly<{
  home: CachedView<TimelineSnapshot>;
  notifications: CachedView<NotificationsSnapshot>;
  profiles: ProfileSet;
}>;

export type ViewCacheStreamEvent =
  | { readonly kind: "Update"; readonly status: Status }
  | { readonly kind: "Delete"; readonly statusId: string }
  | { readonly kind: "Notification"; readonly notification: Notification };

const patchTimeline = (
  snapshot: TimelineSnapshot,
  updated: Status,
): TimelineSnapshot => {
  const statuses = Status.replaceInList(snapshot.statuses, updated);
  return Object.is(statuses, snapshot.statuses) ? snapshot : { ...snapshot, statuses };
};

const patchNotifications = (
  snapshot: NotificationsSnapshot,
  updated: Status,
): NotificationsSnapshot => {
  let changed = false;
  const notifications = snapshot.notifications.map((notification) => {
    const current = Notification.status(notification);
    if (!current) {
      return notification;
    }
    const [next] = Status.replaceInList([current], updated);
    if (Object.is(next, current)) {
      return notification;
    }
    changed = true;
    return Notification.withStatus(notification, next);
  });
  return changed ? { ...snapshot, notifications } : snapshot;
};

const patchProfile = (snapshot: ProfileSnapshot, updated: Status): ProfileSnapshot => {
  const statuses = Status.replaceInList(snapshot.statuses, updated);
  return Object.is(statuses, snapshot.statuses) ? snapshot : { ...snapshot, statuses };
};

const removeStatus = (state: ViewCacheState, statusId: string): ViewCacheState => {
  const home = CachedView.map(state.home, (snapshot) => {
    const statuses = Status.removeById(snapshot.statuses, statusId);
    return Object.is(statuses, snapshot.statuses) ? snapshot : { ...snapshot, statuses };
  });
  const notifications = CachedView.map(state.notifications, (snapshot) => {
    const next = snapshot.notifications.filter((item) => {
      const attached = Notification.status(item);
      return attached === null || !Status.containsId([attached], statusId);
    });
    return next.length === snapshot.notifications.length
      ? snapshot
      : { ...snapshot, notifications: next };
  });
  const profiles = ProfileSet.map(state.profiles, (snapshot) => {
    const statuses = Status.removeById(snapshot.statuses, statusId);
    return Object.is(statuses, snapshot.statuses) ? snapshot : { ...snapshot, statuses };
  });
  if (
    home === state.home &&
    notifications === state.notifications &&
    profiles === state.profiles
  ) {
    return state;
  }
  return { home, notifications, profiles };
};

export const ViewCache = {
  empty: (): ViewCacheState => ({
    home: CachedView.absent(),
    notifications: CachedView.absent(),
    profiles: ProfileSet.empty(),
  }),

  writeHome: (state: ViewCacheState, snapshot: TimelineSnapshot): ViewCacheState => ({
    ...state,
    home: CachedView.present(snapshot),
  }),

  writeNotifications: (state: ViewCacheState, snapshot: NotificationsSnapshot): ViewCacheState => ({
    ...state,
    notifications: CachedView.present(snapshot),
  }),

  writeProfile: (
    state: ViewCacheState,
    accountId: string,
    snapshot: ProfileSnapshot,
  ): ViewCacheState => ({
    ...state,
    profiles: ProfileSet.insert(state.profiles, accountId, snapshot),
  }),

  receivePreloadedProfile: (
    state: ViewCacheState,
    accountId: string,
    snapshot: ProfileSnapshot,
  ): ViewCacheState =>
    ProfileSet.has(state.profiles, accountId)
      ? state
      : ViewCache.writeProfile(state, accountId, snapshot),

  applyHomeUpdate: (
    home: PresentView<TimelineSnapshot>,
    status: Status,
  ): PresentView<TimelineSnapshot> => {
    const statuses = Status.prependUnique(home.value.statuses, status);
    return Object.is(statuses, home.value.statuses)
      ? home
      : CachedView.present({ ...home.value, statuses });
  },

  applyNotification: (
    notifications: PresentView<NotificationsSnapshot>,
    notification: Notification,
  ): PresentView<NotificationsSnapshot> => {
    if (notifications.value.notifications.some((item) => item.id === notification.id)) {
      return notifications;
    }
    return CachedView.present({
      ...notifications.value,
      notifications: [notification, ...notifications.value.notifications],
    });
  },

  applyStreamEvent: (state: ViewCacheState, event: ViewCacheStreamEvent): ViewCacheState => {
    switch (event.kind) {
      case "Update":
        switch (state.home.kind) {
          case "Absent":
            return state;
          case "Present":
            return { ...state, home: ViewCache.applyHomeUpdate(state.home, event.status) };
        }
      case "Notification":
        switch (state.notifications.kind) {
          case "Absent":
            return state;
          case "Present":
            return {
              ...state,
              notifications: ViewCache.applyNotification(state.notifications, event.notification),
            };
        }
      case "Delete":
        return removeStatus(state, event.statusId);
    }
  },

  patchStatus: (state: ViewCacheState, updated: Status): ViewCacheState => {
    const home = CachedView.map(state.home, (snapshot) => patchTimeline(snapshot, updated));
    const notifications = CachedView.map(state.notifications, (snapshot) =>
      patchNotifications(snapshot, updated),
    );
    const profiles = ProfileSet.map(state.profiles, (snapshot) => patchProfile(snapshot, updated));
    if (
      home === state.home &&
      notifications === state.notifications &&
      profiles === state.profiles
    ) {
      return state;
    }
    return { home, notifications, profiles };
  },
} as const;
