import type { AccountRef as AccountRefType } from "@/domain/account/account";
import type { Visibility } from "@/domain/status/visibility";

export type MediaAttachment = Readonly<{
  id: string;
  type: string;
  url: string;
  previewUrl: string;
  description: string | null;
}>;

export type Status = Readonly<{
  id: string;
  createdAt: string;
  content: string;
  spoilerText: string;
  sensitive: boolean;
  visibility: Visibility;
  inReplyToId: string | null;
  repliesCount: number;
  reblogsCount: number;
  favouritesCount: number;
  favourited: boolean;
  reblogged: boolean;
  bookmarked: boolean;
  account: AccountRefType;
  mediaAttachments: ReadonlyArray<MediaAttachment>;
  reblog: Status | null;
}>;

export const Status = {
  displayBody: (status: Status): Status => status.reblog ?? status,

  boostedBy: (status: Status): AccountRefType | null =>
    status.reblog ? status.account : null,
} as const;

export type StatusContext = Readonly<{
  ancestors: ReadonlyArray<Status>;
  descendants: ReadonlyArray<Status>;
}>;
