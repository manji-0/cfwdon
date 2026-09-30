const dateTimeFormat = new Intl.DateTimeFormat("ja-JP", {
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
  hour: "2-digit",
  minute: "2-digit",
});

/**
 * Server timestamps are UTC but often lack a zone (D1 `CURRENT_TIMESTAMP` is
 * `YYYY-MM-DD HH:MM:SS`). `new Date()` would read zone-less values as local
 * time, so append `Z` to any timestamp without an explicit offset.
 */
function parseTimestamp(value: string): Date | null {
  const zoneless = /^\d{4}-\d{2}-\d{2}[ T]\d{2}:\d{2}(:\d{2}(\.\d+)?)?$/.test(value);
  const normalized = zoneless ? `${value.replace(" ", "T")}Z` : value;
  const date = new Date(normalized);
  return Number.isNaN(date.getTime()) ? null : date;
}

export function formatDateTime(value: string | null | undefined): string {
  if (!value) {
    return "—";
  }
  const date = parseTimestamp(value);
  return date ? dateTimeFormat.format(date) : value;
}

export function isoDateTime(value: string | null | undefined): string | undefined {
  if (!value) {
    return undefined;
  }
  return parseTimestamp(value)?.toISOString();
}

export type Tone = "ok" | "warn" | "danger" | "neutral" | "";

export type StatusLabel = { label: string; tone: Tone };

const deliveryStates: Record<string, StatusLabel> = {
  queued: { label: "待機", tone: "neutral" },
  expanded: { label: "展開済み", tone: "neutral" },
  in_flight: { label: "送信中", tone: "" },
  delivered: { label: "配信済み", tone: "ok" },
  failed: { label: "失敗", tone: "danger" },
};

const jobStatuses: Record<string, StatusLabel> = {
  pending: { label: "待機", tone: "neutral" },
  running: { label: "実行中", tone: "" },
  completed: { label: "完了", tone: "ok" },
  failed: { label: "失敗", tone: "danger" },
};

const relayStates: Record<string, StatusLabel> = {
  accepted: { label: "有効", tone: "ok" },
  pending: { label: "承認待ち", tone: "warn" },
  rejected: { label: "拒否", tone: "danger" },
  idle: { label: "無効", tone: "neutral" },
};

const lookup = (table: Record<string, StatusLabel>, value: string): StatusLabel =>
  table[value] ?? { label: value, tone: "neutral" };

export const deliveryState = (value: string): StatusLabel => lookup(deliveryStates, value);
export const jobStatus = (value: string): StatusLabel => lookup(jobStatuses, value);
export const relayState = (value: string): StatusLabel => lookup(relayStates, value);

/** Admin list endpoints cap results at this many rows per source. */
export const ADMIN_LIST_LIMIT = 100;
