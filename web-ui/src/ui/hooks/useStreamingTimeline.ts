import { useEffect } from "react";
import { Status } from "@/domain/status/status";
import {
  StreamingUser,
  type StreamingUserEvent,
} from "@/infrastructure/streaming/mastodon-stream";

export const applyStreamingTimelineEvent = (
  current: ReadonlyArray<Status>,
  event: StreamingUserEvent,
): ReadonlyArray<Status> => {
  switch (event.kind) {
    case "Update":
      return Status.prependUnique(current, event.status);
    case "Delete":
      return Status.removeById(current, event.statusId);
    case "Notification":
    case "Conversation":
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
      if (event.kind === "Notification" || event.kind === "Conversation") {
        return;
      }
      onStatusesChange((current) => applyStreamingTimelineEvent(current, event));
    });
    return () => subscription.close();
  }, [enabled, onStatusesChange]);
};
