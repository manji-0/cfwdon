/** Attach uploaded media ids when publishing statuses. */
export type UploadedMedia = Readonly<{
  id: string;
  type: string;
  url: string;
  previewUrl: string;
}>;
