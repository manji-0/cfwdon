import { type ResultAsync } from "neverthrow";
import type { UploadedMedia } from "@/domain/media/attachment";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import { parseUploadedMedia } from "@/infrastructure/mastodon/parsers/media";
import { mastodonUploadFile } from "@/infrastructure/http/mastodon-fetch";

export const uploadMedia = (file: File): ResultAsync<UploadedMedia, MastodonFetchError> =>
  mastodonUploadFile("/api/v1/media", file).andThen((raw) =>
    parseMastodon(parseUploadedMedia, raw),
  );
