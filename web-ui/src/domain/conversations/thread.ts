import type { Status } from "@/domain/status/status";

export const flattenConversationStatuses = (
  ancestors: ReadonlyArray<Status>,
  focus: Status | null,
  descendants: ReadonlyArray<Status>,
): ReadonlyArray<Status> => (focus ? [...ancestors, focus, ...descendants] : [...ancestors, ...descendants]);

export const belongsToConversationThread = (
  current: ReadonlyArray<Status>,
  status: Status,
): boolean => {
  if (status.visibility.kind !== "Direct") {
    return false;
  }
  if (current.some((item) => item.id === status.id)) {
    return true;
  }
  if (current.length === 0) {
    return true;
  }
  const ids = new Set(current.map((item) => item.id));
  if (status.inReplyToId && ids.has(status.inReplyToId)) {
    return true;
  }
  return current.some((item) => item.inReplyToId === status.id);
};

export const appendConversationStatus = (
  current: ReadonlyArray<Status>,
  status: Status,
): ReadonlyArray<Status> => {
  if (current.some((item) => item.id === status.id)) {
    return current;
  }
  if (!belongsToConversationThread(current, status)) {
    return current;
  }
  return [...current, status];
};
