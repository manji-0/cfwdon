/** Cursor for Mastodon's `max_id` + `limit` paginated collections. */
export type PageQuery = Readonly<{
  maxId?: string;
  limit?: number;
}>;

/** One fetched page plus the cursor for the next (older) one; null means the end. */
export type Page<T> = Readonly<{
  items: ReadonlyArray<T>;
  nextMaxId: string | null;
}>;

/** Append a page, skipping items whose id is already shown. */
export const appendUnique = <T extends Readonly<{ id: string }>>(
  current: ReadonlyArray<T>,
  incoming: ReadonlyArray<T>,
): ReadonlyArray<T> => {
  const seen = new Set(current.map((item) => item.id));
  return [...current, ...incoming.filter((item) => !seen.has(item.id))];
};
