import type { Status } from "@/domain/status/status";

export type PreviewCard = Readonly<{
  url: string;
  title: string;
  description: string;
  type: string;
  providerName: string;
  providerUrl: string;
  image: string | null;
  blurhash: string | null;
}>;

export const PreviewCard = {
  isVisible: (status: Status): boolean =>
    status.card !== null && status.mediaAttachments.length === 0,
} as const;
