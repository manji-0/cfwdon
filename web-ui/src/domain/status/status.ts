import { z } from "zod";
import { AccountRef, type AccountRef as AccountRefType } from "@/domain/account/account";
import { Visibility, VisibilitySchema } from "@/domain/status/visibility";

export type MediaAttachment = Readonly<{
  id: string;
  type: string;
  url: string;
  previewUrl: string;
  description: string | null;
}>;

const MediaAttachmentSchema = z
  .object({
    id: z.string(),
    type: z.string(),
    url: z.string(),
    preview_url: z.string().optional(),
    description: z.string().nullable().optional(),
  })
  .transform(
    (value): MediaAttachment => ({
      id: value.id,
      type: value.type,
      url: value.url,
      previewUrl: value.preview_url ?? value.url,
      description: value.description ?? null,
    }),
  );

type StatusPayload = Readonly<{
  id: string;
  created_at: string;
  content: string;
  spoiler_text: string;
  sensitive: boolean;
  visibility: Visibility;
  in_reply_to_id?: string | null;
  replies_count: number;
  reblogs_count: number;
  favourites_count: number;
  favourited?: boolean;
  reblogged?: boolean;
  account: AccountRefType;
  media_attachments: MediaAttachment[];
  reblog?: StatusPayload | null;
}>;

const StatusPayloadSchema: z.ZodType<StatusPayload, z.ZodTypeDef, unknown> = z.lazy(() =>
  z.object({
    id: z.string().min(1),
    created_at: z.string(),
    content: z.string(),
    spoiler_text: z.string().optional().default(""),
    sensitive: z.boolean().optional().default(false),
    visibility: VisibilitySchema,
    in_reply_to_id: z.string().nullable().optional(),
    replies_count: z.number().optional().default(0),
    reblogs_count: z.number().optional().default(0),
    favourites_count: z.number().optional().default(0),
    favourited: z.boolean().optional(),
    reblogged: z.boolean().optional(),
    account: AccountRef.schema,
    media_attachments: z.array(MediaAttachmentSchema).optional().default([]),
    reblog: StatusPayloadSchema.nullable().optional(),
  }),
);

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
  account: AccountRef;
  mediaAttachments: ReadonlyArray<MediaAttachment>;
  reblog: Status | null;
}>;

const toStatus = (payload: StatusPayload): Status => ({
  id: payload.id,
  createdAt: payload.created_at,
  content: payload.content,
  spoilerText: payload.spoiler_text,
  sensitive: payload.sensitive,
  visibility: payload.visibility,
  inReplyToId: payload.in_reply_to_id ?? null,
  repliesCount: payload.replies_count,
  reblogsCount: payload.reblogs_count,
  favouritesCount: payload.favourites_count,
  favourited: payload.favourited ?? false,
  reblogged: payload.reblogged ?? false,
  account: payload.account,
  mediaAttachments: payload.media_attachments,
  reblog: payload.reblog ? toStatus(payload.reblog) : null,
});

export const Status = {
  schema: StatusPayloadSchema.transform(toStatus),

  displayBody: (status: Status): Status => status.reblog ?? status,

  boostedBy: (status: Status): AccountRefType | null =>
    status.reblog ? status.account : null,
} as const;

export const StatusListSchema = z.array(Status.schema);

export type StatusContext = Readonly<{
  ancestors: ReadonlyArray<Status>;
  descendants: ReadonlyArray<Status>;
}>;

export const StatusContext = {
  schema: z
    .object({
      ancestors: StatusListSchema,
      descendants: StatusListSchema,
    })
    .transform(
      (value): StatusContext => ({
        ancestors: value.ancestors,
        descendants: value.descendants,
      }),
    ),
} as const;
