import type { AccountRef } from "@/domain/account/account";
import { MediaAttachment } from "@/domain/media/attachment";
import type { Poll } from "@/domain/status/poll";
import { PreviewCard } from "@/domain/status/preview-card";
import {
  Status,
  type OriginalStatus,
  type StatusContext,
} from "@/domain/status/status";
import { Visibility } from "@/domain/status/visibility";
import { type } from "arktype";
import { mastodon } from "@/infrastructure/mastodon/parsers/definitions";
import { parsePoll } from "@/infrastructure/mastodon/parsers/poll";

type StatusPayload = {
  id: string;
  created_at: string;
  content: string;
  spoiler_text?: string;
  sensitive?: boolean;
  visibility: "public" | "unlisted" | "private" | "direct";
  in_reply_to_id?: string | null;
  replies_count?: number;
  reblogs_count?: number;
  favourites_count?: number;
  favourited?: boolean;
  reblogged?: boolean;
  bookmarked?: boolean;
  muted?: boolean;
  account: {
    id: string;
    username: string;
    acct: string;
    display_name: string;
    avatar: string;
  };
  media_attachments?: Array<{
    id: string;
    type: string;
    url: string;
    preview_url?: string;
    description?: string | null;
  }>;
  card?: {
    url: string;
    title: string;
    description: string;
    type: string;
    provider_name: string;
    provider_url: string;
    image?: string | null;
    blurhash?: string | null;
  } | null;
  poll?: {
    id: string;
    expires_at?: string | null;
    expired: boolean;
    multiple: boolean;
    votes_count: number;
    voters_count?: number | null;
    voted?: boolean;
    own_votes?: number[];
    options: Array<{ title: string; votes_count?: number | null }>;
  } | null;
  pinned?: boolean;
  edited_at?: string | null;
  quote?: {
    state: string;
    quoted_status?: StatusPayload | null;
  } | null;
  reblog?: StatusPayload | null;
};

const toAccountRef = (account: StatusPayload["account"]): AccountRef => ({
  id: account.id,
  username: account.username,
  acct: account.acct,
  displayName: account.display_name,
  avatar: account.avatar,
});

const toPreviewCard = (card: NonNullable<StatusPayload["card"]>): PreviewCard => ({
  kind: PreviewCard.fromApi(card.type),
  url: card.url,
  title: card.title,
  description: card.description,
  providerName: card.provider_name,
  providerUrl: card.provider_url,
  image: card.image ?? null,
  blurhash: card.blurhash ?? null,
});

const toMediaAttachment = (
  media: NonNullable<StatusPayload["media_attachments"]>[number],
): MediaAttachment => ({
  kind: MediaAttachment.fromApi(media.type),
  id: media.id,
  url: media.url,
  previewUrl: media.preview_url ?? media.url,
  description: media.description ?? null,
});

const toPoll = (poll: NonNullable<StatusPayload["poll"]>): Poll => {
  const parsed = parsePoll(poll);
  if (parsed instanceof type.errors) {
    return {
      id: poll.id,
      expiresAt: poll.expires_at ?? "",
      expired: poll.expired,
      multiple: poll.multiple,
      votesCount: poll.votes_count,
      votersCount: poll.voters_count ?? null,
      voted: poll.voted ?? false,
      ownVotes: poll.own_votes ?? [],
      options: poll.options.map((option) => ({
        title: option.title,
        votesCount: option.votes_count ?? null,
      })),
    };
  }
  return parsed;
};

const toOriginal = (payload: StatusPayload): OriginalStatus => {
  const nested = payload.reblog;
  const source = nested ?? payload;
  return Status.original({
    id: source.id,
    createdAt: source.created_at,
    content: source.content,
    spoilerText: source.spoiler_text ?? "",
    sensitive: source.sensitive ?? false,
    visibility: Visibility.fromApi(source.visibility),
    inReplyToId: source.in_reply_to_id ?? null,
    repliesCount: source.replies_count ?? 0,
    reblogsCount: source.reblogs_count ?? 0,
    favouritesCount: source.favourites_count ?? 0,
    favourited: source.favourited ?? false,
    reblogged: source.reblogged ?? false,
    bookmarked: source.bookmarked ?? false,
    muted: source.muted ?? false,
    account: toAccountRef(source.account),
    mediaAttachments: (source.media_attachments ?? []).map(toMediaAttachment),
    card: source.card ? toPreviewCard(source.card) : null,
    poll: source.poll ? toPoll(source.poll) : null,
    pinned: source.pinned ?? false,
    editedAt: source.edited_at ?? null,
    quote: source.quote
      ? {
          state: source.quote.state,
          quotedStatus: source.quote.quoted_status
            ? {
                id: source.quote.quoted_status.id,
                content: source.quote.quoted_status.content,
                spoilerText: source.quote.quoted_status.spoiler_text ?? "",
                account: toAccountRef(source.quote.quoted_status.account),
              }
            : null,
        }
      : null,
  });
};

const toStatus = (payload: StatusPayload): Status => {
  if (!payload.reblog) {
    return toOriginal(payload);
  }
  return Status.boost({
    id: payload.id,
    createdAt: payload.created_at,
    account: toAccountRef(payload.account),
    original: toOriginal(payload.reblog),
  });
};

const StatusParser = mastodon.type("StatusPayloadApi").pipe(toStatus);
const StatusListParser = type(StatusParser, "[]");

const StatusContextParser = type({
  ancestors: StatusListParser,
  descendants: StatusListParser,
}).pipe(
  (value): StatusContext => ({
    ancestors: value.ancestors,
    descendants: value.descendants,
  }),
);

export const parseStatus = StatusParser;
export const parseStatusList = StatusListParser;
export const parseStatusContext = StatusContextParser;
