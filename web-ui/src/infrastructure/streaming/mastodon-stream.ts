import type { Conversation } from "@/domain/conversations/conversation";
import type { Notification } from "@/domain/notification/notification";
import type { Status } from "@/domain/status/status";
import { isArkError } from "@/infrastructure/mastodon/parse";
import { parseConversation } from "@/infrastructure/mastodon/parsers/conversations";
import { parseNotification } from "@/infrastructure/mastodon/parsers/notification";
import { parseStatus } from "@/infrastructure/mastodon/parsers/status";
import {
  reconnectDelayMs,
  streamingSubscribeMessage,
} from "@/infrastructure/streaming/reconnect";

export type StreamingSubscription = {
  readonly close: () => void;
};

export type StreamingUserEvent =
  | { readonly kind: "Update"; readonly status: Status }
  | { readonly kind: "Delete"; readonly statusId: string }
  | { readonly kind: "Notification"; readonly notification: Notification }
  | { readonly kind: "Conversation"; readonly conversation: Conversation };

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

export type StreamingListener = (event: StreamingUserEvent) => void;

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
    return statusId.length > 0 ? { kind: "Delete", statusId } : null;
  }

  if (eventName === "update") {
    try {
      const status = parseStatus(JSON.parse(data) as unknown);
      return isArkError(status) ? null : { kind: "Update", status };
    } catch {
      return null;
    }
  }

  if (eventName === "notification") {
    try {
      const notification = parseNotification(JSON.parse(data) as unknown);
      return isArkError(notification) ? null : { kind: "Notification", notification };
    } catch {
      return null;
    }
  }

  if (eventName === "conversation") {
    try {
      const conversation = parseConversation(JSON.parse(data) as unknown);
      return isArkError(conversation) ? null : { kind: "Conversation", conversation };
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

const isDocumentHidden = (): boolean =>
  typeof document !== "undefined" && document.visibilityState === "hidden";

const createStreamingHub = () => {
  const listeners = new Set<StreamingListener>();
  let socket: WebSocket | null = null;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let attempt = 0;
  let waitingWhileHidden = false;
  let visibilityBound = false;

  const emit = (event: StreamingUserEvent) => {
    for (const listener of listeners) {
      listener(event);
    }
  };

  const clearReconnect = () => {
    if (reconnectTimer !== null) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
  };

  const disconnectSocket = () => {
    clearReconnect();
    waitingWhileHidden = false;
    if (socket) {
      const current = socket;
      socket = null;
      current.close();
    }
  };

  const scheduleReconnect = () => {
    if (listeners.size === 0 || reconnectTimer !== null) {
      return;
    }
    if (isDocumentHidden()) {
      waitingWhileHidden = true;
      return;
    }
    const delay = reconnectDelayMs(attempt);
    attempt += 1;
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      connect();
    }, delay);
  };

  const connect = () => {
    if (listeners.size === 0 || socket) {
      return;
    }
    if (isDocumentHidden()) {
      waitingWhileHidden = true;
      return;
    }

    try {
      socket = new WebSocket(streamingWebSocketUrl("user"));
    } catch {
      scheduleReconnect();
      return;
    }

    socket.addEventListener("open", () => {
      attempt = 0;
      waitingWhileHidden = false;
      try {
        socket?.send(streamingSubscribeMessage("direct"));
      } catch {
        // Subscribe is best-effort; user stream is already on the upgrade URL.
      }
    });

    socket.addEventListener("message", (messageEvent) => {
      if (typeof messageEvent.data !== "string") {
        return;
      }
      const event = parseStreamingWebSocketMessage(messageEvent.data);
      if (event) {
        emit(event);
      }
    });

    socket.addEventListener("close", () => {
      socket = null;
      if (listeners.size > 0) {
        scheduleReconnect();
      }
    });

    socket.addEventListener("error", () => {
      // Browser fires error then close; reconnect is handled on close.
    });
  };

  const onVisibilityChange = () => {
    if (isDocumentHidden()) {
      clearReconnect();
      if (listeners.size > 0 && !socket) {
        waitingWhileHidden = true;
      }
      return;
    }
    if (waitingWhileHidden && listeners.size > 0 && !socket) {
      waitingWhileHidden = false;
      connect();
    }
  };

  const bindVisibility = () => {
    if (visibilityBound || typeof document === "undefined") {
      return;
    }
    document.addEventListener("visibilitychange", onVisibilityChange);
    visibilityBound = true;
  };

  const unbindVisibility = () => {
    if (!visibilityBound || typeof document === "undefined") {
      return;
    }
    document.removeEventListener("visibilitychange", onVisibilityChange);
    visibilityBound = false;
  };

  return {
    subscribe: (onEvent: StreamingListener): StreamingSubscription => {
      listeners.add(onEvent);
      bindVisibility();
      if (listeners.size === 1) {
        connect();
      }
      return {
        close: () => {
          listeners.delete(onEvent);
          if (listeners.size === 0) {
            unbindVisibility();
            attempt = 0;
            disconnectSocket();
          }
        },
      };
    },
  };
};

/**
 * Shared authenticated stream hub.
 *
 * One WebSocket to `/api/v1/streaming?stream=user`, then a `direct` subscribe
 * so the session Stream Hub DO fans out both channels. Listeners share the
 * socket via refcount; the last `close()` tears it down.
 */
export const StreamingUser = createStreamingHub();
