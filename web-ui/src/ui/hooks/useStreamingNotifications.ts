import { useEffect } from "react";
import type { Notification } from "@/domain/notification/notification";
import {
  StreamingUser,
  type StreamingUserEvent,
} from "@/infrastructure/streaming/mastodon-stream";

export const applyStreamingNotificationEvent = (
  current: ReadonlyArray<Notification>,
  event: StreamingUserEvent,
): ReadonlyArray<Notification> => {
  if (event.kind !== "notification") {
    return current;
  }
  if (current.some((item) => item.id === event.notification.id)) {
    return current;
  }
  return [event.notification, ...current];
};

/** Subscribe to the user stream and prepend notification events while enabled. */
export const useStreamingNotifications = (
  enabled: boolean,
  onNotificationsChange: (
    updater: (current: ReadonlyArray<Notification>) => ReadonlyArray<Notification>,
  ) => void,
) => {
  useEffect(() => {
    if (!enabled) {
      return undefined;
    }
    const subscription = StreamingUser.subscribe((event) => {
      if (event.kind !== "notification") {
        return;
      }
      onNotificationsChange((current) => applyStreamingNotificationEvent(current, event));
    });
    return () => subscription.close();
  }, [enabled, onNotificationsChange]);
};
