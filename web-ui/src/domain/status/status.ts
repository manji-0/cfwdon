import type { AccountRef as AccountRefType } from "@/domain/account/account";
import type { PreviewCard } from "@/domain/status/preview-card";
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
  card: PreviewCard | null;
  reblog: Status | null;
}>;

export const Status = {
  displayBody: (status: Status): Status => status.reblog ?? status,

  boostedBy: (status: Status): AccountRefType | null =>
    status.reblog ? status.account : null,

  replaceInList: (statuses: ReadonlyArray<Status>, updated: Status): ReadonlyArray<Status> =>
    statuses.map((item) => {
      const body = item.reblog ?? item;
      if (body.id === updated.id) {
        return item.reblog ? { ...item, reblog: updated } : updated;
      }
      return item;
    }),

  containsId: (statuses: ReadonlyArray<Status>, statusId: string): boolean =>
    statuses.some((item) => item.id === statusId || item.reblog?.id === statusId),

  prependUnique: (statuses: ReadonlyArray<Status>, incoming: Status): ReadonlyArray<Status> =>
    Status.containsId(statuses, incoming.id) ? statuses : [incoming, ...statuses],
} as const;

export type StatusContext = Readonly<{
  ancestors: ReadonlyArray<Status>;
  descendants: ReadonlyArray<Status>;
}>;
