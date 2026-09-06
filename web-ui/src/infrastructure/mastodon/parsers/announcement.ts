import type { Announcement } from "@/domain/announcements/announcement";
import { type } from "arktype";

export const parseAnnouncement = type({
  id: "string>0",
  content: "string",
  "read?": "boolean",
  "published_at?": "string | null",
  "updated_at?": "string | null",
  "starts_at?": "string | null",
  "ends_at?": "string | null",
  "all_day?": "boolean",
  "mentions?": "unknown[]",
  "statuses?": "unknown[]",
  "tags?": "unknown[]",
  "emojis?": "unknown[]",
  "reactions?": "unknown[]",
}).pipe(
  (value): Announcement => ({
    id: value.id,
    content: value.content,
    read: value.read ?? false,
    publishedAt: value.published_at ?? null,
  }),
);

export const parseAnnouncementList = type(parseAnnouncement, "[]");
