/** Default page size for Mastodon timeline and collection fetches. */
export const TIMELINE_PAGE_LIMIT = 20;

/** True when a fetched page is full, so another page may exist. */
export const pageHasMore = (fetchedCount: number, limit = TIMELINE_PAGE_LIMIT): boolean =>
  fetchedCount >= limit;
