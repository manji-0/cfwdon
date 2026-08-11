import { z } from "zod";

export type AccountCredentials = Readonly<{
  id: string;
  displayName: string;
  note: string;
  avatar: string;
  username: string;
  acct: string;
  source: Readonly<{
    privacy: string;
    sensitive: boolean;
    language: string | null;
    quotePolicy: string;
  }>;
}>;

export const AccountCredentials = {
  schema: z
    .object({
      id: z.string().min(1),
      display_name: z.string(),
      note: z.string(),
      avatar: z.string(),
      username: z.string().min(1),
      acct: z.string().min(1),
      source: z.object({
        privacy: z.string(),
        sensitive: z.boolean(),
        language: z.string().nullable(),
        quote_policy: z.string(),
      }),
    })
    .transform(
      (value): AccountCredentials => ({
        id: value.id,
        displayName: value.display_name,
        note: value.note,
        avatar: value.avatar,
        username: value.username,
        acct: value.acct,
        source: {
          privacy: value.source.privacy,
          sensitive: value.source.sensitive,
          language: value.source.language,
          quotePolicy: value.source.quote_policy,
        },
      }),
    ),
} as const;
