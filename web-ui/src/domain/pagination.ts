/** Cursor for Mastodon's `max_id` + `limit` paginated collections. */
export type PageQuery = Readonly<{
  maxId?: string;
  limit?: number;
}>;
