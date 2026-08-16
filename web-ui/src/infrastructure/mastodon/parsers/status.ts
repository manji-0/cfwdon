import { type } from "arktype";
import type { AccountRef } from "@/domain/account/account";
import type { PreviewCard } from "@/domain/status/preview-card";
import type { MediaAttachment, Status, StatusContext } from "@/domain/status/status";
import { Visibility } from "@/domain/status/visibility";
import { mastodon } from "@/infrastructure/mastodon/parsers/definitions";

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
  url: card.url,
  title: card.title,
  description: card.description,
  type: card.type,
  providerName: card.provider_name,
  providerUrl: card.provider_url,
  image: card.image ?? null,
  blurhash: card.blurhash ?? null,
});

const toMediaAttachment = (media: NonNullable<StatusPayload["media_attachments"]>[number]): MediaAttachment => ({
  id: media.id,
  type: media.type,
  url: media.url,
  previewUrl: media.preview_url ?? media.url,
  description: media.description ?? null,
});

const toStatus = (payload: StatusPayload): Status => ({
  id: payload.id,
  createdAt: payload.created_at,
  content: payload.content,
  spoilerText: payload.spoiler_text ?? "",
  sensitive: payload.sensitive ?? false,
  visibility: Visibility.fromApi(payload.visibility),
  inReplyToId: payload.in_reply_to_id ?? null,
  repliesCount: payload.replies_count ?? 0,
  reblogsCount: payload.reblogs_count ?? 0,
  favouritesCount: payload.favourites_count ?? 0,
  favourited: payload.favourited ?? false,
  reblogged: payload.reblogged ?? false,
  bookmarked: payload.bookmarked ?? false,
  account: toAccountRef(payload.account),
  mediaAttachments: (payload.media_attachments ?? []).map(toMediaAttachment),
  card: payload.card ? toPreviewCard(payload.card) : null,
  reblog: payload.reblog ? toStatus(payload.reblog) : null,
});

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
