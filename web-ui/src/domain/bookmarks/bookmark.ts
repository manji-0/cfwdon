import type { Status } from "@/domain/status/status";

/** Bookmark statuses via `/api/v1/bookmarks` (returns a status array). */
export type BookmarkList = ReadonlyArray<Status>;
