/** TODO(Phase 4): Subscribe to `/api/v1/streaming` home timeline events. */
export type StreamingEvent = Readonly<{
  event: "update" | "delete" | "notification";
  payload: unknown;
}>;

export type StreamingSubscription = Readonly<{
  close: () => void;
}>;

export const StreamingHome = {
  /** TODO(Phase 4): Open EventSource/WebSocket and dispatch parsed timeline events. */
  subscribe: (
    _onEvent: (event: StreamingEvent) => void,
  ): StreamingSubscription => ({
    close: () => undefined,
  }),
} as const;
