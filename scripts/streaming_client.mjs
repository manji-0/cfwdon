#!/usr/bin/env node

import process from "node:process";
import { setTimeout as delay } from "node:timers/promises";

const DEFAULT_BASE_URL = "http://127.0.0.1:8787";
const DEFAULT_DURATION_MS = 60_000;
const DEFAULT_IDLE_TIMEOUT_MS = 15_000;
const DEFAULT_RECONNECT_DELAY_MS = 1_000;

function usage() {
  return `Usage: node scripts/streaming_client.mjs [options]

Probe cfwdon Mastodon streaming API stability with either SSE or WebSocket.

Options:
  --base-url URL          Instance base URL. Default: ${DEFAULT_BASE_URL}
  --transport sse|ws      Streaming transport. Default: sse
  --stream NAME           Mastodon stream name. Default: public
  --tag TAG               Tag for hashtag streams.
  --list ID               List ID for list streams.
  --token TOKEN           OAuth bearer token. Also sent as WS protocol.
  --access-token TOKEN    Send token as access_token query param.
  --ws-subscribe          Send Mastodon multiplex subscribe JSON after WS open.
  --header NAME:VALUE     Extra request header. Repeatable for SSE only.
  --duration SECONDS      Total probe duration. Default: 60
  --idle-timeout SECONDS  Reconnect after no bytes/messages. Default: 15
  --reconnect-delay MS    Delay between reconnect attempts. Default: 1000
  --verbose               Print each received event/comment.
  --help                  Show this help.

Examples:
  node scripts/streaming_client.mjs --base-url http://127.0.0.1:8787 --stream public --duration 30
  node scripts/streaming_client.mjs --base-url https://example.com --transport ws --token "$TOKEN" --stream user
`;
}

function parseArgs(argv) {
  const options = {
    baseUrl: DEFAULT_BASE_URL,
    transport: "sse",
    stream: "public",
    tag: undefined,
    list: undefined,
    token: undefined,
    accessToken: undefined,
    wsSubscribe: false,
    headers: [],
    durationMs: DEFAULT_DURATION_MS,
    idleTimeoutMs: DEFAULT_IDLE_TIMEOUT_MS,
    reconnectDelayMs: DEFAULT_RECONNECT_DELAY_MS,
    verbose: false,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    const nextValue = () => {
      const value = argv[index + 1];
      if (!value || value.startsWith("--")) {
        throw new Error(`Missing value for ${arg}`);
      }
      index += 1;
      return value;
    };

    if (arg === "--help" || arg === "-h") {
      options.help = true;
    } else if (arg === "--base-url") {
      options.baseUrl = nextValue();
    } else if (arg === "--transport") {
      options.transport = nextValue();
    } else if (arg === "--stream") {
      options.stream = nextValue();
    } else if (arg === "--tag") {
      options.tag = nextValue();
    } else if (arg === "--list") {
      options.list = nextValue();
    } else if (arg === "--token") {
      options.token = nextValue();
    } else if (arg === "--access-token") {
      options.accessToken = nextValue();
    } else if (arg === "--ws-subscribe") {
      options.wsSubscribe = true;
    } else if (arg === "--header") {
      options.headers.push(nextValue());
    } else if (arg === "--duration") {
      options.durationMs = secondsToMs(nextValue(), "--duration");
    } else if (arg === "--idle-timeout") {
      options.idleTimeoutMs = secondsToMs(nextValue(), "--idle-timeout");
    } else if (arg === "--reconnect-delay") {
      options.reconnectDelayMs = integerValue(nextValue(), "--reconnect-delay");
    } else if (arg === "--verbose") {
      options.verbose = true;
    } else {
      throw new Error(`Unknown option: ${arg}`);
    }
  }

  if (!["sse", "ws"].includes(options.transport)) {
    throw new Error("--transport must be sse or ws");
  }
  if (!options.stream.trim()) {
    throw new Error("--stream must not be empty");
  }
  if (options.durationMs <= 0) {
    throw new Error("--duration must be greater than zero");
  }
  if (options.idleTimeoutMs <= 0) {
    throw new Error("--idle-timeout must be greater than zero");
  }
  if (options.reconnectDelayMs < 0) {
    throw new Error("--reconnect-delay must be zero or greater");
  }

  return options;
}

function secondsToMs(value, name) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) {
    throw new Error(`${name} must be a number of seconds`);
  }
  return Math.round(parsed * 1000);
}

function integerValue(value, name) {
  const parsed = Number(value);
  if (!Number.isInteger(parsed)) {
    throw new Error(`${name} must be an integer`);
  }
  return parsed;
}

function streamingUrl(options) {
  const url = new URL("/api/v1/streaming", withTrailingSlash(options.baseUrl));
  url.searchParams.set("stream", options.stream);
  if (options.tag) {
    url.searchParams.set("tag", options.tag);
  }
  if (options.list) {
    url.searchParams.set("list", options.list);
  }
  if (options.accessToken) {
    url.searchParams.set("access_token", options.accessToken);
  }
  return url;
}

function websocketUrl(options) {
  const url = streamingUrl(options);
  if (url.protocol === "https:") {
    url.protocol = "wss:";
  } else if (url.protocol === "http:") {
    url.protocol = "ws:";
  } else if (!["ws:", "wss:"].includes(url.protocol)) {
    throw new Error(`Cannot derive WebSocket URL from protocol ${url.protocol}`);
  }
  return url;
}

function withTrailingSlash(value) {
  return value.endsWith("/") ? value : `${value}/`;
}

function extraHeaders(headerSpecs) {
  const headers = new Headers();
  for (const spec of headerSpecs) {
    const separator = spec.indexOf(":");
    if (separator <= 0) {
      throw new Error(`Invalid --header value: ${spec}`);
    }
    const name = spec.slice(0, separator).trim();
    const value = spec.slice(separator + 1).trim();
    if (!name) {
      throw new Error(`Invalid --header value: ${spec}`);
    }
    headers.set(name, value);
  }
  return headers;
}

function newMetrics(options) {
  return {
    startedAt: Date.now(),
    deadline: Date.now() + options.durationMs,
    attempts: 0,
    reconnects: 0,
    failures: 0,
    bytes: 0,
    comments: 0,
    events: 0,
    eventsByName: new Map(),
    closes: [],
    lastMessageAt: undefined,
    firstMessageAt: undefined,
    maxGapMs: 0,
  };
}

function recordActivity(metrics, bytes = 0) {
  const now = Date.now();
  if (metrics.lastMessageAt) {
    metrics.maxGapMs = Math.max(metrics.maxGapMs, now - metrics.lastMessageAt);
  }
  metrics.lastMessageAt = now;
  metrics.firstMessageAt ??= now;
  metrics.bytes += bytes;
}

function recordEvent(metrics, eventName) {
  metrics.events += 1;
  metrics.eventsByName.set(eventName, (metrics.eventsByName.get(eventName) ?? 0) + 1);
}

function recordClose(metrics, close) {
  metrics.closes.push(close);
}

function shouldContinue(metrics) {
  return Date.now() < metrics.deadline;
}

function remainingMs(metrics) {
  return Math.max(0, metrics.deadline - Date.now());
}

async function probeSse(options, metrics) {
  const url = streamingUrl(options);
  const headers = extraHeaders(options.headers);
  headers.set("Accept", "text/event-stream");
  if (options.token) {
    headers.set("Authorization", `Bearer ${options.token}`);
  }

  while (shouldContinue(metrics)) {
    metrics.attempts += 1;
    const attempt = metrics.attempts;
    const controller = new AbortController();
    const idleTimer = makeIdleTimer(controller, options, metrics);
    const deadlineTimer = setTimeout(() => controller.abort("duration elapsed"), remainingMs(metrics));

    try {
      console.error(`[sse] attempt ${attempt} connecting to ${url}`);
      const response = await fetch(url, {
        headers,
        signal: controller.signal,
      });
      if (!response.ok) {
        const body = await response.text();
        throw new Error(`HTTP ${response.status}: ${body.slice(0, 500)}`);
      }
      console.error(`[sse] attempt ${attempt} connected status=${response.status}`);
      await readSseBody(response, options, metrics, idleTimer);
      recordClose(metrics, { attempt, reason: "body ended" });
    } catch (error) {
      if (isDurationElapsed(error) || !shouldContinue(metrics)) {
        recordClose(metrics, { attempt, reason: "duration elapsed" });
        break;
      }
      metrics.failures += 1;
      recordClose(metrics, { attempt, reason: errorMessage(error) });
      console.error(`[sse] attempt ${attempt} failed: ${errorMessage(error)}`);
    } finally {
      clearTimeout(deadlineTimer);
      idleTimer.clear();
    }

    if (shouldContinue(metrics)) {
      metrics.reconnects += 1;
      await delay(Math.min(options.reconnectDelayMs, remainingMs(metrics)));
    }
  }
}

function makeIdleTimer(controller, options, metrics) {
  let timer;
  const reset = () => {
    clearTimeout(timer);
    timer = setTimeout(() => {
      controller.abort(`idle timeout after ${options.idleTimeoutMs}ms`);
    }, options.idleTimeoutMs);
  };
  reset();
  return {
    reset,
    clear: () => clearTimeout(timer),
    record: (bytes) => {
      recordActivity(metrics, bytes);
      reset();
    },
  };
}

async function readSseBody(response, options, metrics, idleTimer) {
  const reader = response.body?.getReader();
  if (!reader) {
    throw new Error("Response body is not readable");
  }

  const decoder = new TextDecoder();
  let buffer = "";
  while (shouldContinue(metrics)) {
    const { value, done } = await reader.read();
    if (done) {
      return;
    }
    const byteLength = value.byteLength;
    idleTimer.record(byteLength);
    buffer += decoder.decode(value, { stream: true });

    let separator;
    while ((separator = findSseSeparator(buffer)) !== -1) {
      const rawEvent = buffer.slice(0, separator.index);
      buffer = buffer.slice(separator.index + separator.length);
      parseSseEvent(rawEvent, options, metrics);
    }
  }
}

function findSseSeparator(value) {
  const lf = value.indexOf("\n\n");
  const crlf = value.indexOf("\r\n\r\n");
  if (lf === -1) {
    return crlf === -1 ? -1 : { index: crlf, length: 4 };
  }
  if (crlf === -1) {
    return { index: lf, length: 2 };
  }
  return lf < crlf ? { index: lf, length: 2 } : { index: crlf, length: 4 };
}

function parseSseEvent(rawEvent, options, metrics) {
  if (!rawEvent.trim()) {
    return;
  }

  const lines = rawEvent.split(/\r?\n/);
  const comments = [];
  let eventName = "message";
  const data = [];

  for (const line of lines) {
    if (line.startsWith(":")) {
      comments.push(line.slice(1).trimStart());
      continue;
    }
    const colon = line.indexOf(":");
    const field = colon === -1 ? line : line.slice(0, colon);
    const value = colon === -1 ? "" : line.slice(colon + 1).replace(/^ /, "");
    if (field === "event") {
      eventName = value;
    } else if (field === "data") {
      data.push(value);
    }
  }

  if (comments.length > 0 && data.length === 0) {
    metrics.comments += comments.length;
    if (options.verbose) {
      console.log(`[comment] ${comments.join(" | ")}`);
    }
    return;
  }

  recordEvent(metrics, eventName);
  if (options.verbose) {
    console.log(`[event:${eventName}] ${data.join("\n").slice(0, 1000)}`);
  }
}

async function probeWebSocket(options, metrics) {
  if (typeof WebSocket === "undefined") {
    throw new Error("This Node runtime does not expose WebSocket. Use Node 22+.");
  }
  if (options.headers.length > 0) {
    console.error("[ws] ignoring --header values; the WebSocket API only supports protocols here");
  }

  const url = websocketUrl(options);
  while (shouldContinue(metrics)) {
    metrics.attempts += 1;
    const attempt = metrics.attempts;
    try {
      console.error(`[ws] attempt ${attempt} connecting to ${url}`);
      await connectWebSocket(url, options, metrics, attempt);
    } catch (error) {
      if (!shouldContinue(metrics)) {
        recordClose(metrics, { attempt, reason: "duration elapsed" });
        break;
      }
      metrics.failures += 1;
      recordClose(metrics, { attempt, reason: errorMessage(error) });
      console.error(`[ws] attempt ${attempt} failed: ${errorMessage(error)}`);
    }

    if (shouldContinue(metrics)) {
      metrics.reconnects += 1;
      await delay(Math.min(options.reconnectDelayMs, remainingMs(metrics)));
    }
  }
}

function connectWebSocket(url, options, metrics, attempt) {
  return new Promise((resolve, reject) => {
    let settled = false;
    let opened = false;
    const protocols = options.token ? [options.token] : [];
    const websocket = new WebSocket(url, protocols);
    const failTimer = setTimeout(() => {
      websocket.close(4000, "connect timeout");
      reject(new Error("connect timeout"));
    }, Math.min(options.idleTimeoutMs, remainingMs(metrics) || options.idleTimeoutMs));
    let idleTimer;
    const deadlineTimer = setTimeout(() => {
      websocket.close(1000, "duration elapsed");
    }, remainingMs(metrics));

    const finish = (value, isError = false) => {
      if (settled) {
        return;
      }
      settled = true;
      clearTimeout(failTimer);
      clearTimeout(idleTimer);
      clearTimeout(deadlineTimer);
      if (isError) {
        reject(value);
      } else {
        resolve(value);
      }
    };
    const resetIdle = () => {
      clearTimeout(idleTimer);
      idleTimer = setTimeout(() => {
        websocket.close(4001, "idle timeout");
      }, options.idleTimeoutMs);
    };

    websocket.addEventListener("open", () => {
      opened = true;
      clearTimeout(failTimer);
      resetIdle();
      console.error(`[ws] attempt ${attempt} connected`);
      if (options.wsSubscribe) {
        websocket.send(JSON.stringify(wsSubscribePayload(options)));
      }
    });

    websocket.addEventListener("message", (message) => {
      const data = typeof message.data === "string" ? message.data : "";
      recordActivity(metrics, Buffer.byteLength(data));
      resetIdle();
      try {
        parseWebSocketMessage(data, options, metrics);
      } catch (error) {
        websocket.close(4002, "streaming error");
        finish(error, true);
      }
    });

    websocket.addEventListener("error", () => {
      if (!opened) {
        finish(new Error("websocket error before open"), true);
      }
    });

    websocket.addEventListener("close", (event) => {
      recordClose(metrics, {
        attempt,
        code: event.code,
        reason: event.reason || "closed",
        clean: event.wasClean,
      });
      if (!opened && !settled) {
        finish(new Error(`closed before open code=${event.code} reason=${event.reason}`), true);
      } else {
        finish();
      }
    });
  });
}

function parseWebSocketMessage(data, options, metrics) {
  if (!data.trim()) {
    return;
  }
  if (data.startsWith("{")) {
    parseWebSocketJsonMessage(data, options, metrics);
    return;
  }
  if (data.startsWith(":")) {
    metrics.comments += 1;
    if (options.verbose) {
      console.log(`[comment] ${data.trim()}`);
    }
    return;
  }

  const eventMatch = data.match(/^event: ([^\n\r]+)/m);
  const eventName = eventMatch?.[1] ?? "message";
  recordEvent(metrics, eventName);
  if (options.verbose) {
    console.log(`[event:${eventName}] ${data.slice(0, 1000)}`);
  }
}

function wsSubscribePayload(options) {
  const payload = {
    type: "subscribe",
    stream: options.stream,
  };
  if (options.tag) {
    payload.tag = options.tag;
  }
  if (options.list) {
    payload.list = options.list;
  }
  return payload;
}

function parseWebSocketJsonMessage(data, options, metrics) {
  let message;
  try {
    message = JSON.parse(data);
  } catch {
    if (options.verbose) {
      console.log(`[message] ${data.slice(0, 1000)}`);
    }
    return;
  }
  if (message.error) {
    throw new Error(`streaming error ${message.status ?? ""}: ${message.error}`);
  }
  const eventName = message.event ?? "message";
  recordEvent(metrics, eventName);
  if (options.verbose) {
    console.log(`[event:${eventName}] ${JSON.stringify(message).slice(0, 1000)}`);
  }
}

function errorMessage(error) {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

function isDurationElapsed(error) {
  return errorMessage(error) === "duration elapsed";
}

function printSummary(options, metrics) {
  const now = Date.now();
  const elapsedMs = now - metrics.startedAt;
  const connectedMs = metrics.firstMessageAt ? now - metrics.firstMessageAt : 0;
  const eventSummary =
    [...metrics.eventsByName.entries()]
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([name, count]) => `${name}=${count}`)
      .join(", ") || "none";
  const lastClose = metrics.closes.at(-1);

  console.log("");
  console.log("Streaming probe summary");
  console.log(`  url: ${options.transport === "ws" ? websocketUrl(options) : streamingUrl(options)}`);
  console.log(`  transport: ${options.transport}`);
  console.log(`  elapsed: ${(elapsedMs / 1000).toFixed(1)}s`);
  console.log(`  attempts: ${metrics.attempts}`);
  console.log(`  reconnects: ${metrics.reconnects}`);
  console.log(`  failures: ${metrics.failures}`);
  console.log(`  bytes: ${metrics.bytes}`);
  console.log(`  comments: ${metrics.comments}`);
  console.log(`  events: ${metrics.events}`);
  console.log(`  events_by_name: ${eventSummary}`);
  console.log(`  first_message_after: ${metrics.firstMessageAt ? `${metrics.firstMessageAt - metrics.startedAt}ms` : "none"}`);
  console.log(`  observed_after_first_message: ${(connectedMs / 1000).toFixed(1)}s`);
  console.log(`  max_gap: ${metrics.maxGapMs}ms`);
  console.log(`  last_close: ${lastClose ? JSON.stringify(lastClose) : "none"}`);
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) {
    console.log(usage());
    return;
  }

  const metrics = newMetrics(options);
  try {
    if (options.transport === "ws") {
      await probeWebSocket(options, metrics);
    } else {
      await probeSse(options, metrics);
    }
  } finally {
    printSummary(options, metrics);
  }

  if (metrics.failures > 0 || metrics.firstMessageAt === undefined) {
    process.exitCode = 1;
  }
}

main().catch((error) => {
  console.error(errorMessage(error));
  process.exit(1);
});
