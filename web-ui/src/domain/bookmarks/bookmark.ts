import { Status, StatusListSchema } from "@/domain/status/status";

/** TODO(Phase 5): Bookmark statuses via `/api/v1/bookmarks` (returns a status array). */
export type BookmarkList = ReadonlyArray<Status>;

export const BookmarkList = {
  schema: StatusListSchema,
} as const;
