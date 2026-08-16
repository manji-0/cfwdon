import { type } from "arktype";
import { MediaAttachment, type UploadedMedia } from "@/domain/media/attachment";

export const parseUploadedMedia = type({
  id: "string>0",
  type: "string",
  url: "string",
  "preview_url?": "string",
}).pipe(
  (value): UploadedMedia => ({
    kind: MediaAttachment.fromApi(value.type),
    id: value.id,
    url: value.url,
    previewUrl: value.preview_url ?? value.url,
  }),
);
