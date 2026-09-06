import { type ResultAsync } from "neverthrow";
import type { Announcement } from "@/domain/announcements/announcement";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonFetchJson, mastodonPostJson } from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import { parseAnnouncementList } from "@/infrastructure/mastodon/parsers/announcement";

export const fetchAnnouncements = (): ResultAsync<
  ReadonlyArray<Announcement>,
  MastodonFetchError
> =>
  mastodonFetchJson("/api/v1/announcements").andThen((raw) =>
    parseMastodon(parseAnnouncementList, raw),
  );

export const dismissAnnouncement = (id: string): ResultAsync<void, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/announcements/${encodeURIComponent(id)}/dismiss`, {}).map(
    () => undefined,
  );
