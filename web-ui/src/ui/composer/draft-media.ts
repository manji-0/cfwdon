/** UI-layer composer media item (keeps browser `File` out of domain). */
export type ComposerMediaItem = Readonly<{
  localId: string;
  file: File;
  previewUrl: string;
  status: "uploading" | "ready" | "failed";
  mediaId?: string;
  errorMessage?: string;
}>;

export const ComposerMedia = {
  maxAttachments: 4,
  accept: "image/*,video/*,audio/*",
} as const;
