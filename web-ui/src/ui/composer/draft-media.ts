import type { Result } from "neverthrow";

/** UI-layer composer media (keeps browser `File` out of domain). */
export type ComposerMediaUploading = Readonly<{
  kind: "Uploading";
  localId: string;
  file: File;
  previewUrl: string;
}>;

export type ComposerMediaReady = Readonly<{
  kind: "Ready";
  localId: string;
  file: File;
  previewUrl: string;
  mediaId: string;
}>;

export type ComposerMediaFailed = Readonly<{
  kind: "Failed";
  localId: string;
  file: File;
  previewUrl: string;
  message: string;
}>;

export type ComposerMediaItem = ComposerMediaUploading | ComposerMediaReady | ComposerMediaFailed;

export const COMPOSER_MEDIA_MAX_ATTACHMENTS = 4;

export const ComposerMedia = {
  maxAttachments: COMPOSER_MEDIA_MAX_ATTACHMENTS,
  accept: "image/*,video/*,audio/*",

  createLocalId: (): string =>
    typeof crypto !== "undefined" && "randomUUID" in crypto
      ? crypto.randomUUID()
      : `${Date.now()}-${Math.random().toString(16).slice(2)}`,

  uploading: (localId: string, file: File, previewUrl: string): ComposerMediaUploading => ({
    kind: "Uploading",
    localId,
    file,
    previewUrl,
  }),

  markReady: (item: ComposerMediaUploading, mediaId: string): ComposerMediaReady => ({
    kind: "Ready",
    localId: item.localId,
    file: item.file,
    previewUrl: item.previewUrl,
    mediaId,
  }),

  markFailed: (item: ComposerMediaUploading, message: string): ComposerMediaFailed => ({
    kind: "Failed",
    localId: item.localId,
    file: item.file,
    previewUrl: item.previewUrl,
    message,
  }),

  isUploading: (item: ComposerMediaItem) => item.kind === "Uploading",
  isReady: (item: ComposerMediaItem) => item.kind === "Ready",

  readyIds: (items: ReadonlyArray<ComposerMediaItem>): ReadonlyArray<string> =>
    items.filter(ComposerMedia.isReady).map((item) => item.mediaId),

  hasUploading: (items: ReadonlyArray<ComposerMediaItem>) => items.some(ComposerMedia.isUploading),

  append: (
    items: ReadonlyArray<ComposerMediaItem>,
    incoming: ReadonlyArray<ComposerMediaItem>,
  ): ReadonlyArray<ComposerMediaItem> => {
    const room = ComposerMedia.maxAttachments - items.length;
    if (room <= 0) {
      return items;
    }
    return [...items, ...incoming.slice(0, room)];
  },

  complete: (
    items: ReadonlyArray<ComposerMediaItem>,
    localId: string,
    outcome: Result<string, string>,
  ): ReadonlyArray<ComposerMediaItem> =>
    items.map((item) => {
      if (item.localId !== localId || item.kind !== "Uploading") {
        return item;
      }
      return outcome.isOk()
        ? ComposerMedia.markReady(item, outcome.value)
        : ComposerMedia.markFailed(item, outcome.error);
    }),

  remove: (items: ReadonlyArray<ComposerMediaItem>, localId: string): ReadonlyArray<ComposerMediaItem> =>
    items.filter((item) => item.localId !== localId),

  member: (items: ReadonlyArray<ComposerMediaItem>, localId: string): ComposerMediaItem | undefined =>
    items.find((item) => item.localId === localId),
} as const;
