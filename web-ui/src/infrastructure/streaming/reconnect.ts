const BASE_MS = 1_000;
const CAP_MS = 30_000;
const MAX_JITTER_MS = 250;

/** Exponential delay: 1s, 2s, 4s, … capped at 30s, plus optional jitter. */
export const reconnectDelayMs = (
  attempt: number,
  random: () => number = Math.random,
): number => {
  const exponent = Math.max(0, attempt);
  const exponential = Math.min(CAP_MS, BASE_MS * 2 ** exponent);
  const jitter = Math.floor(random() * MAX_JITTER_MS);
  return exponential + jitter;
};

export const streamingSubscribeMessage = (stream: string): string =>
  JSON.stringify({ type: "subscribe", stream });
