import { type } from "arktype";
import type { UploadedMedia } from "@/domain/media/attachment";

export const parseUploadedMedia = type({
  id: "string>0",
  type: "string",
  url: "string",
  "preview_url?": "string",
}).pipe(
  (value): UploadedMedia => ({
    id: value.id,
    type: value.type,
    url: value.url,
    previewUrl: value.preview_url ?? value.url,
  }),
);
