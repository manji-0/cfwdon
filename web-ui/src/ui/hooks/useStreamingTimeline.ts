import { useEffect } from "react";
import type { Status } from "@/domain/status/status";
import {
  StreamingUser,
  type StreamingUserEvent,
} from "@/infrastructure/streaming/mastodon-stream";

export const applyStreamingTimelineEvent = (
  current: ReadonlyArray<Status>,
  event: StreamingUserEvent,
): ReadonlyArray<Status> => {
  switch (event.kind) {
    case "update": {
      if (current.some((status) => status.id === event.status.id)) {
        return current;
      }
      return [event.status, ...current];
    }
    case "delete":
      return current.filter((status) => status.id !== event.statusId);
    case "notification":
      return current;
  }
};

/** Subscribe to the user stream and merge timeline updates while enabled. */
export const useStreamingTimeline = (
  enabled: boolean,
  onStatusesChange: (updater: (current: ReadonlyArray<Status>) => ReadonlyArray<Status>) => void,
) => {
  useEffect(() => {
    if (!enabled) {
      return undefined;
    }
    const subscription = StreamingUser.subscribe((event) => {
      if (event.kind === "notification") {
        return;
      }
      onStatusesChange((current) => applyStreamingTimelineEvent(current, event));
    });
    return () => subscription.close();
  }, [enabled, onStatusesChange]);
};
