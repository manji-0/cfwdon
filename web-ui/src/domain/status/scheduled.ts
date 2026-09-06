export const SCHEDULED_STATUS_MIN_OFFSET_MS = 5 * 60 * 1000;

export type ScheduledStatus = Readonly<{
  id: string;
  scheduledAt: string;
  text: string;
  spoilerText: string;
  visibility: string;
  sensitive: boolean;
}>;

const pad = (value: number): string => String(value).padStart(2, "0");

export const ScheduledAt = {
  minOffsetMs: SCHEDULED_STATUS_MIN_OFFSET_MS,

  defaultLocalValue: (now = Date.now()): string =>
    ScheduledAt.toLocalValue(new Date(now + 60 * 60 * 1000).toISOString()),

  minLocalValue: (now = Date.now()): string =>
    ScheduledAt.toLocalValue(new Date(now + SCHEDULED_STATUS_MIN_OFFSET_MS + 60_000).toISOString()),

  toRfc3339: (localValue: string): string | null => {
    if (!localValue) {
      return null;
    }
    const date = new Date(localValue);
    if (Number.isNaN(date.getTime())) {
      return null;
    }
    return date.toISOString();
  },

  toLocalValue: (iso: string): string => {
    const date = new Date(iso);
    if (Number.isNaN(date.getTime())) {
      return "";
    }
    return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
  },

  isFarEnough: (iso: string, now = Date.now()): boolean => {
    const date = new Date(iso);
    return !Number.isNaN(date.getTime()) && date.getTime() > now + SCHEDULED_STATUS_MIN_OFFSET_MS;
  },
} as const;
