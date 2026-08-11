import type { Notification } from "@/domain/notification/notification";
import type { Status } from "@/domain/status/status";
import { isArkError } from "@/infrastructure/mastodon/parse";
import { parseNotification } from "@/infrastructure/mastodon/parsers/notification";
import { parseStatus } from "@/infrastructure/mastodon/parsers/status";

/** Keepalive / reconnect when the socket closes unexpectedly. */
const RECONNECT_MS = 3_000;

export type StreamingSubscription = {
  readonly close: () => void;
};

export type StreamingUserEvent =
  | { readonly kind: "update"; readonly status: Status }
  | { readonly kind: "delete"; readonly statusId: string }
  | { readonly kind: "notification"; readonly notification: Notification };

/**
 * Mastodon WS event shape from Stream Hub DO / worker poll fallback:
 * `{ "stream": ["user"], "event": "update", "payload": "<json string>" }`
 */
export type StreamingWebSocketMessage = {
  readonly event?: string;
  readonly payload?: string;
  readonly error?: string;
  readonly stream?: ReadonlyArray<string>;
};

export const streamingWebSocketUrl = (
  stream: string,
  location: Pick<Location, "protocol" | "host"> = window.location,
): string => {
  const protocol = location.protocol === "https:" ? "wss:" : "ws:";
  const params = new URLSearchParams({ stream });
  return `${protocol}//${location.host}/api/v1/streaming?${params}`;
};

export const parseStreamingPayload = (
  eventName: string,
  data: string,
): StreamingUserEvent | null => {
  if (eventName === "delete") {
    const statusId = data.trim();
    return statusId.length > 0 ? { kind: "delete", statusId } : null;
  }

  if (eventName === "update") {
    try {
      const status = parseStatus(JSON.parse(data) as unknown);
      return isArkError(status) ? null : { kind: "update", status };
    } catch {
      return null;
    }
  }

  if (eventName === "notification") {
    try {
      const notification = parseNotification(JSON.parse(data) as unknown);
      return isArkError(notification) ? null : { kind: "notification", notification };
    } catch {
      return null;
    }
  }

  return null;
};

export const parseStreamingWebSocketMessage = (
  raw: string,
): StreamingUserEvent | null => {
  let message: StreamingWebSocketMessage;
  try {
    message = JSON.parse(raw) as StreamingWebSocketMessage;
  } catch {
    return null;
  }

  if (typeof message.error === "string" && message.error.length > 0) {
    return null;
  }

  const eventName = typeof message.event === "string" ? message.event : "";
  if (eventName.length === 0) {
    return null;
  }

  const payload = typeof message.payload === "string" ? message.payload : "";
  return parseStreamingPayload(eventName, payload);
};

/**
 * Subscribe to the authenticated `user` stream via WebSocket.
 *
 * The worker upgrades this socket to the Stream Hub Durable Object
 * (`upgrade_stream_hub_websocket` → session hub for the viewer), so events
 * are pushed from DO rather than relying on the SSE D1-poll path.
 *
 * Soft-fail: connection errors and parse failures are ignored; callers keep
 * REST-loaded data. Reconnects after unexpected close until `close()`.
 */
export const StreamingUser = {
  subscribe: (onEvent: (event: StreamingUserEvent) => void): StreamingSubscription => {
    let closed = false;
    let socket: WebSocket | null = null;
    let reconnectTimer: ReturnType<typeof setTimeout> | null = null;

    const clearReconnect = () => {
      if (reconnectTimer !== null) {
        clearTimeout(reconnectTimer);
        reconnectTimer = null;
      }
    };

    const scheduleReconnect = () => {
      if (closed || reconnectTimer !== null) {
        return;
      }
      reconnectTimer = setTimeout(() => {
        reconnectTimer = null;
        connect();
      }, RECONNECT_MS);
    };

    const connect = () => {
      if (closed) {
        return;
      }

      try {
        socket = new WebSocket(streamingWebSocketUrl("user"));
      } catch {
        scheduleReconnect();
        return;
      }

      socket.addEventListener("message", (messageEvent) => {
        if (closed || typeof messageEvent.data !== "string") {
          return;
        }
        const event = parseStreamingWebSocketMessage(messageEvent.data);
        if (event) {
          onEvent(event);
        }
      });

      socket.addEventListener("close", () => {
        socket = null;
        if (!closed) {
          scheduleReconnect();
        }
      });

      socket.addEventListener("error", () => {
        // Browser fires error then close; reconnect is handled on close.
      });
    };

    connect();

    return {
      close: () => {
        closed = true;
        clearReconnect();
        if (socket) {
          socket.close();
          socket = null;
        }
      },
    };
  },
};
