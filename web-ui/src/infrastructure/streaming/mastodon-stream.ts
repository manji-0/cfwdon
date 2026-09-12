import { ForegroundResume } from "@/domain/cache/foreground-resume";
import type { Conversation } from "@/domain/conversations/conversation";
import { assertNever } from "@/domain/never";
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
export type StreamingResumeListener = () => void;

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

export const createStreamingHub = () => {
  const listeners = new Set<StreamingListener>();
  const resumeListeners = new Set<StreamingResumeListener>();
  let socket: WebSocket | null = null;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let attempt = 0;
  let visibilityBound = false;
  let hiddenAt: number | null = null;
  let lastCatchUpAt: number | null = null;

  const emit = (event: StreamingUserEvent) => {
    for (const listener of listeners) {
      listener(event);
    }
  };

  const notifyCatchUp = () => {
    const now = Date.now();
    if (!ForegroundResume.shouldEmit(lastCatchUpAt, now)) {
      return;
    }
    lastCatchUpAt = now;
    for (const listener of resumeListeners) {
      listener();
    }
  };

  const hasHolders = (): boolean => listeners.size > 0 || resumeListeners.size > 0;

  const clearReconnect = () => {
    if (reconnectTimer !== null) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
  };

  const disconnectSocket = () => {
    clearReconnect();
    if (socket) {
      const current = socket;
      socket = null;
      current.close();
    }
  };

  const releaseIfIdle = () => {
    if (hasHolders()) {
      return;
    }
    unbindVisibility();
    attempt = 0;
    hiddenAt = null;
    disconnectSocket();
  };

  const scheduleReconnect = () => {
    if (listeners.size === 0 || reconnectTimer !== null) {
      return;
    }
    if (isDocumentHidden()) {
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
      return;
    }

    try {
      socket = new WebSocket(streamingWebSocketUrl("user"));
    } catch {
      scheduleReconnect();
      return;
    }

    const opened = socket;

    socket.addEventListener("open", () => {
      if (socket !== opened) {
        return;
      }
      attempt = 0;
      try {
        opened.send(streamingSubscribeMessage("direct"));
      } catch {
        // Subscribe is best-effort; user stream is already on the upgrade URL.
      }
    });

    socket.addEventListener("message", (messageEvent) => {
      if (socket !== opened || typeof messageEvent.data !== "string") {
        return;
      }
      const event = parseStreamingWebSocketMessage(messageEvent.data);
      if (event) {
        emit(event);
      }
    });

    socket.addEventListener("close", () => {
      if (socket !== opened) {
        return;
      }
      socket = null;
      if (listeners.size > 0) {
        scheduleReconnect();
      }
    });

    socket.addEventListener("error", () => {
      // Browser fires error then close; reconnect is handled on close.
    });
  };

  const becomeVisible = (resume: ForegroundResume) => {
    hiddenAt = null;
    switch (resume.kind) {
      case "KeepStream":
        if (listeners.size > 0 && !socket) {
          attempt = 0;
          connect();
        }
        return;
      case "CatchUp":
        attempt = 0;
        if (socket) {
          disconnectSocket();
        }
        connect();
        notifyCatchUp();
        return;
      default:
        return assertNever(resume);
    }
  };

  const onVisibilityChange = () => {
    if (isDocumentHidden()) {
      hiddenAt = Date.now();
      clearReconnect();
      return;
    }
    const hiddenMs = hiddenAt === null ? 0 : Date.now() - hiddenAt;
    becomeVisible(ForegroundResume.forHiddenDuration(hiddenMs));
  };

  const onPageShow = (event: Event) => {
    if (!("persisted" in event) || !(event as PageTransitionEvent).persisted) {
      return;
    }
    becomeVisible(ForegroundResume.forBfcacheRestore());
  };

  const bindVisibility = () => {
    if (visibilityBound || typeof document === "undefined") {
      return;
    }
    document.addEventListener("visibilitychange", onVisibilityChange);
    if (typeof window !== "undefined") {
      window.addEventListener("pageshow", onPageShow);
    }
    visibilityBound = true;
  };

  const unbindVisibility = () => {
    if (!visibilityBound || typeof document === "undefined") {
      return;
    }
    document.removeEventListener("visibilitychange", onVisibilityChange);
    if (typeof window !== "undefined") {
      window.removeEventListener("pageshow", onPageShow);
    }
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
            disconnectSocket();
          }
          releaseIfIdle();
        },
      };
    },

    subscribeResume: (onResume: StreamingResumeListener): StreamingSubscription => {
      resumeListeners.add(onResume);
      bindVisibility();
      return {
        close: () => {
          resumeListeners.delete(onResume);
          releaseIfIdle();
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
