import type { AccountRef } from "@/domain/account/account";
import type { MediaAttachment } from "@/domain/media/attachment";
import { assertNever } from "@/domain/never";
import type { PreviewCard } from "@/domain/status/preview-card";
import type { Visibility } from "@/domain/status/visibility";

type StatusBody = Readonly<{
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
  account: AccountRef;
  mediaAttachments: ReadonlyArray<MediaAttachment>;
  card: PreviewCard | null;
}>;

export type OriginalStatus = StatusBody &
  Readonly<{
    kind: "Original";
  }>;

export type BoostedStatus = Readonly<{
  kind: "Boost";
  id: string;
  createdAt: string;
  account: AccountRef;
  original: OriginalStatus;
}>;

export type Status = OriginalStatus | BoostedStatus;

export type StatusContext = Readonly<{
  ancestors: ReadonlyArray<Status>;
  descendants: ReadonlyArray<Status>;
}>;

export const Status = {
  original: (fields: StatusBody): OriginalStatus => ({
    kind: "Original",
    ...fields,
  }),

  boost: (fields: Readonly<{
    id: string;
    createdAt: string;
    account: AccountRef;
    original: OriginalStatus;
  }>): BoostedStatus => ({
    kind: "Boost",
    ...fields,
  }),

  displayBody: (status: Status): OriginalStatus => {
    switch (status.kind) {
      case "Original":
        return status;
      case "Boost":
        return status.original;
      default:
        return assertNever(status);
    }
  },

  boostedBy: (status: Status): AccountRef | null => {
    switch (status.kind) {
      case "Original":
        return null;
      case "Boost":
        return status.account;
      default:
        return assertNever(status);
    }
  },

  withBody: (status: Status, original: OriginalStatus): Status => {
    switch (status.kind) {
      case "Original":
        return original;
      case "Boost":
        return Object.is(status.original, original) ? status : { ...status, original };
      default:
        return assertNever(status);
    }
  },

  visibleCard: (status: Status): PreviewCard | null => {
    const body = Status.displayBody(status);
    return body.card !== null && body.mediaAttachments.length === 0 ? body.card : null;
  },

  containsId: (statuses: ReadonlyArray<Status>, statusId: string): boolean =>
    statuses.some((item) => item.id === statusId || Status.displayBody(item).id === statusId),

  findByBodyId: (statuses: ReadonlyArray<Status>, statusId: string): Status | undefined =>
    statuses.find((item) => Status.displayBody(item).id === statusId),

  replaceInList: (statuses: ReadonlyArray<Status>, updated: Status): ReadonlyArray<Status> => {
    const nextBody = Status.displayBody(updated);
    let changed = false;
    const next = statuses.map((item) => {
      if (Status.displayBody(item).id !== nextBody.id) {
        return item;
      }
      const replaced = Status.withBody(item, nextBody);
      if (!Object.is(replaced, item)) {
        changed = true;
      }
      return replaced;
    });
    return changed ? next : statuses;
  },

  prependUnique: (statuses: ReadonlyArray<Status>, incoming: Status): ReadonlyArray<Status> =>
    Status.containsId(statuses, incoming.id) ? statuses : [incoming, ...statuses],

  removeById: (statuses: ReadonlyArray<Status>, statusId: string): ReadonlyArray<Status> => {
    const next = statuses.filter(
      (item) => item.id !== statusId && Status.displayBody(item).id !== statusId,
    );
    return next.length === statuses.length ? statuses : next;
  },
} as const;
