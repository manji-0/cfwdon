import { z } from "zod";

/** Attach uploaded media ids when publishing statuses. */
export type UploadedMedia = Readonly<{
  id: string;
  type: string;
  url: string;
  previewUrl: string;
}>;

export const UploadedMedia = {
  schema: z
    .object({
      id: z.string().min(1),
      type: z.string(),
      url: z.string(),
      preview_url: z.string().optional(),
    })
    .transform(
      (value): UploadedMedia => ({
        id: value.id,
        type: value.type,
        url: value.url,
        previewUrl: value.preview_url ?? value.url,
      }),
    ),
} as const;
