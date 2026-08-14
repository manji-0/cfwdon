import { Notification } from "@/domain/notification/notification";
import type { Status } from "@/domain/status/status";
import { Status as StatusModel } from "@/domain/status/status";
import { CachedView, type CachedView as CachedSlot, type PresentView } from "./cached-view";
import { ProfileSet, type ProfileSet as ProfileSetState, type ProfileSnapshot } from "./profile-set";

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
  home: CachedSlot<TimelineSnapshot>;
  notifications: CachedSlot<NotificationsSnapshot>;
  profiles: ProfileSetState;
}>;

export type ViewCacheStreamEvent =
  | { readonly kind: "update"; readonly status: Status }
  | { readonly kind: "delete"; readonly statusId: string }
  | { readonly kind: "notification"; readonly notification: Notification };

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
  ): PresentView<TimelineSnapshot> =>
    CachedView.present({
      ...home.value,
      statuses: StatusModel.prependUnique(home.value.statuses, status),
    }),

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
      case "update":
        switch (state.home.kind) {
          case "Absent":
            return state;
          case "Present":
            return { ...state, home: ViewCache.applyHomeUpdate(state.home, event.status) };
        }
      case "notification":
        switch (state.notifications.kind) {
          case "Absent":
            return state;
          case "Present":
            return {
              ...state,
              notifications: ViewCache.applyNotification(state.notifications, event.notification),
            };
        }
      case "delete":
        return removeStatus(state, event.statusId);
    }
  },

  patchStatus: (state: ViewCacheState, updated: Status): ViewCacheState => ({
    home: CachedView.map(state.home, (snapshot) => ({
      ...snapshot,
      statuses: StatusModel.replaceInList(snapshot.statuses, updated),
    })),
    notifications: CachedView.map(state.notifications, (snapshot) => ({
      ...snapshot,
      notifications: snapshot.notifications.map((notification) => {
        const current = Notification.status(notification);
        if (!current) {
          return notification;
        }
        const [next] = StatusModel.replaceInList([current], updated);
        return next === current ? notification : Notification.withStatus(notification, next);
      }),
    })),
    profiles: ProfileSet.map(state.profiles, (snapshot) => ({
      ...snapshot,
      statuses: StatusModel.replaceInList(snapshot.statuses, updated),
    })),
  }),
} as const;

const removeStatus = (state: ViewCacheState, statusId: string): ViewCacheState => ({
  home: CachedView.map(state.home, (snapshot) => ({
    ...snapshot,
    statuses: snapshot.statuses.filter((item) => item.id !== statusId && item.reblog?.id !== statusId),
  })),
  notifications: CachedView.map(state.notifications, (snapshot) => ({
    ...snapshot,
    notifications: snapshot.notifications.filter(
      (item) => Notification.status(item)?.id !== statusId,
    ),
  })),
  profiles: ProfileSet.map(state.profiles, (snapshot) => ({
    ...snapshot,
    statuses: snapshot.statuses.filter((item) => item.id !== statusId && item.reblog?.id !== statusId),
  })),
});
