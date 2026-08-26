import { assertNever } from "@/domain/never";

export type MediaAttachmentKind = "Image" | "Video" | "Gifv" | "Audio" | "Unknown";

export type MediaAttachment = Readonly<{
  kind: MediaAttachmentKind;
  id: string;
  url: string;
  previewUrl: string;
  description: string | null;
}>;

export type UploadedMedia = Readonly<{
  kind: MediaAttachmentKind;
  id: string;
  url: string;
  previewUrl: string;
  description: string | null;
}>;

export const MediaAttachment = {
  fromApi: (value: string): MediaAttachmentKind => {
    switch (value) {
      case "image":
        return "Image";
      case "video":
        return "Video";
      case "gifv":
        return "Gifv";
      case "audio":
        return "Audio";
      default:
        return "Unknown";
    }
  },

  isVisual: (media: MediaAttachment): boolean => {
    switch (media.kind) {
      case "Image":
      case "Gifv":
        return true;
      case "Video":
      case "Audio":
      case "Unknown":
        return false;
      default:
        return assertNever(media.kind);
    }
  },

  label: (media: MediaAttachment): string => {
    switch (media.kind) {
      case "Image":
        return "画像";
      case "Video":
        return "動画";
      case "Gifv":
        return "GIF";
      case "Audio":
        return "音声";
      case "Unknown":
        return "メディア";
      default:
        return assertNever(media.kind);
    }
  },
} as const;
