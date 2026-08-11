import { describe, expect, it } from "vitest";
import {
  parseStreamingPayload,
  parseStreamingWebSocketMessage,
  streamingWebSocketUrl,
} from "@/infrastructure/streaming/mastodon-stream";

const sampleStatus = {
  id: "status-1",
  created_at: "2026-01-01T00:00:00.000Z",
  content: "<p>hello</p>",
  visibility: "public",
  account: {
    id: "acct-1",
    username: "alice",
    acct: "alice",
    display_name: "Alice",
    avatar: "https://example.test/a.png",
  },
};

const sampleNotification = {
  id: "notif-1",
  type: "favourite",
  group_key: "g1",
  created_at: "2026-01-01T00:00:00.000Z",
  account: sampleStatus.account,
  status: sampleStatus,
};

describe("streamingWebSocketUrl", () => {
  it("builds a same-origin wss URL for the user stream", () => {
    expect(
      streamingWebSocketUrl("user", { protocol: "https:", host: "example.test" }),
    ).toBe("wss://example.test/api/v1/streaming?stream=user");
  });

  it("uses ws on http origins", () => {
    expect(
      streamingWebSocketUrl("user", { protocol: "http:", host: "localhost:5173" }),
    ).toBe("ws://localhost:5173/api/v1/streaming?stream=user");
  });
});

describe("parseStreamingPayload", () => {
  it("parses update events into statuses", () => {
    const event = parseStreamingPayload("update", JSON.stringify(sampleStatus));
    expect(event?.kind).toBe("update");
    if (event?.kind === "update") {
      expect(event.status.id).toBe("status-1");
      expect(event.status.account.displayName).toBe("Alice");
    }
  });

  it("parses delete events as status ids", () => {
    expect(parseStreamingPayload("delete", "status-1")).toEqual({
      kind: "delete",
      statusId: "status-1",
    });
  });

  it("parses notification events", () => {
    const event = parseStreamingPayload("notification", JSON.stringify(sampleNotification));
    expect(event?.kind).toBe("notification");
    if (event?.kind === "notification") {
      expect(event.notification.id).toBe("notif-1");
      expect(event.notification.type).toBe("favourite");
    }
  });

  it("ignores unknown or invalid payloads", () => {
    expect(parseStreamingPayload("filters_changed", "{}")).toBeNull();
    expect(parseStreamingPayload("update", "not-json")).toBeNull();
  });
});

describe("parseStreamingWebSocketMessage", () => {
  it("parses Mastodon/Stream Hub fanout JSON", () => {
    const event = parseStreamingWebSocketMessage(
      JSON.stringify({
        stream: ["user"],
        event: "update",
        payload: JSON.stringify(sampleStatus),
      }),
    );
    expect(event?.kind).toBe("update");
    if (event?.kind === "update") {
      expect(event.status.id).toBe("status-1");
    }
  });

  it("ignores error frames and malformed JSON", () => {
    expect(
      parseStreamingWebSocketMessage(JSON.stringify({ error: "Unauthorized", status: 401 })),
    ).toBeNull();
    expect(parseStreamingWebSocketMessage("not-json")).toBeNull();
  });
});
