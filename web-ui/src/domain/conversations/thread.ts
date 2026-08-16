import { Status } from "@/domain/status/status";

const threadIds = (current: ReadonlyArray<Status>): ReadonlySet<string> =>
  new Set(current.map((item) => item.id));

export const flattenConversationStatuses = (
  ancestors: ReadonlyArray<Status>,
  focus: Status | null,
  descendants: ReadonlyArray<Status>,
): ReadonlyArray<Status> => (focus ? [...ancestors, focus, ...descendants] : [...ancestors, ...descendants]);

export const belongsToConversationThread = (
  current: ReadonlyArray<Status>,
  status: Status,
): boolean => {
  if (Status.displayBody(status).visibility.kind !== "Direct") {
    return false;
  }
  if (current.length === 0) {
    return true;
  }
  const ids = threadIds(current);
  if (ids.has(status.id)) {
    return true;
  }
  const inReplyToId = Status.displayBody(status).inReplyToId;
  if (inReplyToId && ids.has(inReplyToId)) {
    return true;
  }
  return current.some((item) => Status.displayBody(item).inReplyToId === status.id);
};

export const appendConversationStatus = (
  current: ReadonlyArray<Status>,
  status: Status,
): ReadonlyArray<Status> => {
  if (Status.displayBody(status).visibility.kind !== "Direct") {
    return current;
  }
  const ids = threadIds(current);
  if (ids.has(status.id)) {
    return current;
  }
  if (current.length === 0) {
    return [...current, status];
  }
  const inReplyToId = Status.displayBody(status).inReplyToId;
  if (inReplyToId && ids.has(inReplyToId)) {
    return [...current, status];
  }
  if (current.some((item) => Status.displayBody(item).inReplyToId === status.id)) {
    return [...current, status];
  }
  return current;
};
