import { useEffect } from "react";
import { StreamingHome } from "@/infrastructure/streaming/mastodon-stream";

/** TODO(Phase 4): Merge streaming events into timeline state on the home page. */
export const useStreamingTimeline = (enabled: boolean, onEvent: (payload: unknown) => void) => {
  useEffect(() => {
    if (!enabled) {
      return undefined;
    }
    const subscription = StreamingHome.subscribe((event) => {
      onEvent(event.payload);
    });
    return () => subscription.close();
  }, [enabled, onEvent]);
};
