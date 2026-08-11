import { scope, type } from "arktype";
import type { AccountRef } from "@/domain/account/account";
import type { MediaAttachment, Status, StatusContext } from "@/domain/status/status";
import { Visibility } from "@/domain/status/visibility";

const mastodonStatusScope = scope({
  VisibilityApi: "'public' | 'unlisted' | 'private' | 'direct'",
  AccountRefApi: {
    id: "string>0",
    username: "string>0",
    acct: "string>0",
    display_name: "string",
    avatar: "string",
  },
  MediaAttachmentApi: {
    id: "string",
    type: "string",
    url: "string",
    "preview_url?": "string",
    "description?": "string | null",
  },
  StatusPayloadApi: {
    id: "string>0",
    created_at: "string",
    content: "string",
    "spoiler_text?": "string",
    "sensitive?": "boolean",
    visibility: "VisibilityApi",
    "in_reply_to_id?": "string | null",
    "replies_count?": "number",
    "reblogs_count?": "number",
    "favourites_count?": "number",
    "favourited?": "boolean",
    "reblogged?": "boolean",
    account: "AccountRefApi",
    "media_attachments?": "MediaAttachmentApi[]",
    "reblog?": "StatusPayloadApi | null",
  },
});

const toAccountRef = (account: {
  id: string;
  username: string;
  acct: string;
  display_name: string;
  avatar: string;
}): AccountRef => ({
  id: account.id,
  username: account.username,
  acct: account.acct,
  displayName: account.display_name,
  avatar: account.avatar,
});

const toMediaAttachment = (media: {
  id: string;
  type: string;
  url: string;
  preview_url?: string;
  description?: string | null;
}): MediaAttachment => ({
  id: media.id,
  type: media.type,
  url: media.url,
  previewUrl: media.preview_url ?? media.url,
  description: media.description ?? null,
});

const toStatus = (payload: {
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
  reblog?: typeof payload | null;
}): Status => ({
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
  account: toAccountRef(payload.account),
  mediaAttachments: (payload.media_attachments ?? []).map(toMediaAttachment),
  reblog: payload.reblog ? toStatus(payload.reblog) : null,
});

const StatusParser = mastodonStatusScope.type("StatusPayloadApi").pipe(toStatus);
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
