/** @vitest-environment happy-dom */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FOREGROUND_CATCH_UP_AFTER_MS } from "@/domain/cache/foreground-resume";
import { createStreamingHub } from "@/infrastructure/streaming/mastodon-stream";

type Listener = (event: Event) => void;

class FakeWebSocket {
  static instances: FakeWebSocket[] = [];
  readonly url: string;
  readonly sent: string[] = [];
  readonly listeners = new Map<string, Listener[]>();
  readyState = 0;

  constructor(url: string) {
    this.url = url;
    FakeWebSocket.instances.push(this);
  }

  addEventListener(type: string, listener: Listener): void {
    const current = this.listeners.get(type) ?? [];
    current.push(listener);
    this.listeners.set(type, current);
  }

  send(data: string): void {
    this.sent.push(data);
  }

  close(): void {
    this.readyState = 3;
    this.emit("close");
  }

  emit(type: string): void {
    for (const listener of this.listeners.get(type) ?? []) {
      listener(new Event(type));
    }
  }
}

const setHidden = (hidden: boolean): void => {
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    get: () => (hidden ? "hidden" : "visible"),
  });
  document.dispatchEvent(new Event("visibilitychange"));
};

describe("createStreamingHub visibility resume", () => {
  const originalWebSocket = globalThis.WebSocket;
  const subscriptions: Array<{ close: () => void }> = [];

  const subscribe = (hub: ReturnType<typeof createStreamingHub>) => {
    const stream = hub.subscribe(() => undefined);
    subscriptions.push(stream);
    return stream;
  };

  beforeEach(() => {
    FakeWebSocket.instances = [];
    vi.stubGlobal("WebSocket", FakeWebSocket);
    setHidden(false);
    vi.useFakeTimers();
    vi.setSystemTime(1_000_000);
  });

  afterEach(() => {
    while (subscriptions.length > 0) {
      subscriptions.pop()?.close();
    }
    vi.useRealTimers();
    vi.unstubAllGlobals();
    globalThis.WebSocket = originalWebSocket;
    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      value: "visible",
    });
  });

  it("reconnects immediately when the tab returns after the socket dropped while hidden", () => {
    const hub = createStreamingHub();
    subscribe(hub);
    expect(FakeWebSocket.instances).toHaveLength(1);

    setHidden(true);
    FakeWebSocket.instances[0]?.close();
    expect(FakeWebSocket.instances).toHaveLength(1);

    setHidden(false);
    expect(FakeWebSocket.instances).toHaveLength(2);
  });

  it("force-reconnects a live socket after a long background and notifies resume listeners", () => {
    const hub = createStreamingHub();
    const resumes: number[] = [];
    subscribe(hub);
    const resume = hub.subscribeResume(() => {
      resumes.push(Date.now());
    });
    subscriptions.push(resume);
    const first = FakeWebSocket.instances[0];
    expect(first).toBeDefined();

    setHidden(true);
    vi.setSystemTime(1_000_000 + FOREGROUND_CATCH_UP_AFTER_MS);
    setHidden(false);

    expect(FakeWebSocket.instances).toHaveLength(2);
    expect(FakeWebSocket.instances[1]).not.toBe(first);
    expect(resumes).toEqual([1_000_000 + FOREGROUND_CATCH_UP_AFTER_MS]);
  });

  it("does not tear down a live socket after a short background", () => {
    const hub = createStreamingHub();
    subscribe(hub);
    const first = FakeWebSocket.instances[0];

    setHidden(true);
    vi.setSystemTime(1_000_000 + FOREGROUND_CATCH_UP_AFTER_MS - 1);
    setHidden(false);

    expect(FakeWebSocket.instances).toHaveLength(1);
    expect(FakeWebSocket.instances[0]).toBe(first);
  });

  it("catches up on back-forward cache restore", () => {
    const hub = createStreamingHub();
    const resumes: number[] = [];
    subscribe(hub);
    const resume = hub.subscribeResume(() => {
      resumes.push(1);
    });
    subscriptions.push(resume);
    expect(FakeWebSocket.instances).toHaveLength(1);

    window.dispatchEvent(new Event("pageshow"));
    expect(FakeWebSocket.instances).toHaveLength(1);
    expect(resumes).toEqual([]);

    const restored = new Event("pageshow") as PageTransitionEvent;
    Object.defineProperty(restored, "persisted", { configurable: true, value: true });
    window.dispatchEvent(restored);
    expect(FakeWebSocket.instances).toHaveLength(2);
    expect(resumes).toEqual([1]);
  });
});
