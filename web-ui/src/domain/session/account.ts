import { z } from "zod";

export type AccountSummary = Readonly<{
  id: string;
  username: string;
  displayName: string;
  acct: string;
  avatar: string;
  instanceName: string;
}>;

export const AccountSummary = {
  schema: z
    .object({
      id: z.string().min(1),
      username: z.string().min(1),
      display_name: z.string(),
      acct: z.string().min(1),
      avatar: z.string(),
      instance_name: z.string().min(1),
    })
    .transform(
      (value): AccountSummary => ({
        id: value.id,
        username: value.username,
        displayName: value.display_name,
        acct: value.acct,
        avatar: value.avatar,
        instanceName: value.instance_name,
      }),
    ),
} as const;
