import { errAsync, okAsync, type ResultAsync } from "neverthrow";
import { UploadedMedia } from "@/domain/media/attachment";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonUploadFile } from "@/infrastructure/http/mastodon-fetch";

export const uploadMedia = (file: File): ResultAsync<UploadedMedia, MastodonFetchError> =>
  mastodonUploadFile("/api/v1/media", file).andThen((raw) => {
    const parsed = UploadedMedia.schema.safeParse(raw);
    if (!parsed.success) {
      return errAsync({ kind: "ValidationError" } as const);
    }
    return okAsync(parsed.data);
  });
