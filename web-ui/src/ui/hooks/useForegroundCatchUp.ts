import { useEffect } from "react";
import { StreamingUser } from "@/infrastructure/streaming/mastodon-stream";

/** Run `onCatchUp` when a long-backgrounded tab returns to the foreground. */
export const useForegroundCatchUp = (onCatchUp: () => void): void => {
  useEffect(() => {
    const subscription = StreamingUser.subscribeResume(onCatchUp);
    return () => subscription.close();
  }, [onCatchUp]);
};
