import { z } from "zod";

export type AccountRef = Readonly<{
  id: string;
  username: string;
  acct: string;
  displayName: string;
  avatar: string;
}>;

export const AccountRef = {
  schema: z
    .object({
      id: z.string().min(1),
      username: z.string().min(1),
      acct: z.string().min(1),
      display_name: z.string(),
      avatar: z.string(),
    })
    .transform(
      (value): AccountRef => ({
        id: value.id,
        username: value.username,
        acct: value.acct,
        displayName: value.display_name,
        avatar: value.avatar,
      }),
    ),
} as const;

export type AccountProfile = Readonly<{
  id: string;
  username: string;
  acct: string;
  displayName: string;
  avatar: string;
  header: string;
  note: string;
  followersCount: number;
  followingCount: number;
  statusesCount: number;
  locked: boolean;
}>;

export const AccountProfile = {
  schema: z
    .object({
      id: z.string().min(1),
      username: z.string().min(1),
      acct: z.string().min(1),
      display_name: z.string(),
      avatar: z.string(),
      header: z.string(),
      note: z.string(),
      followers_count: z.number(),
      following_count: z.number(),
      statuses_count: z.number(),
      locked: z.boolean(),
    })
    .transform(
      (value): AccountProfile => ({
        id: value.id,
        username: value.username,
        acct: value.acct,
        displayName: value.display_name,
        avatar: value.avatar,
        header: value.header,
        note: value.note,
        followersCount: value.followers_count,
        followingCount: value.following_count,
        statusesCount: value.statuses_count,
        locked: value.locked,
      }),
    ),
} as const;
