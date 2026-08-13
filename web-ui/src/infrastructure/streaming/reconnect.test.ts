import { describe, expect, it } from "vitest";
import {
  reconnectDelayMs,
  streamingSubscribeMessage,
} from "@/infrastructure/streaming/reconnect";

describe("reconnectDelayMs", () => {
  it("starts at 1s and doubles until the 30s cap", () => {
    const noJitter = () => 0;
    expect(reconnectDelayMs(0, noJitter)).toBe(1_000);
    expect(reconnectDelayMs(1, noJitter)).toBe(2_000);
    expect(reconnectDelayMs(2, noJitter)).toBe(4_000);
    expect(reconnectDelayMs(5, noJitter)).toBe(30_000);
    expect(reconnectDelayMs(8, noJitter)).toBe(30_000);
  });

  it("adds jitter from the random source", () => {
    expect(reconnectDelayMs(0, () => 0.4)).toBe(1_000 + Math.floor(0.4 * 250));
  });
});

describe("streamingSubscribeMessage", () => {
  it("serializes a Mastodon subscribe frame", () => {
    expect(JSON.parse(streamingSubscribeMessage("direct"))).toEqual({
      type: "subscribe",
      stream: "direct",
    });
  });
});
