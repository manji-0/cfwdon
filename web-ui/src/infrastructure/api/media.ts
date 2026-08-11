import { type ResultAsync } from "neverthrow";
import type { UploadedMedia } from "@/domain/media/attachment";
import { notImplemented, type MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";

/** TODO(Phase 1): POST multipart form to `/api/v1/media` and parse `UploadedMedia`. */
export const uploadMedia = (_file: File): ResultAsync<UploadedMedia, MastodonFetchError> =>
  notImplemented("media upload");
