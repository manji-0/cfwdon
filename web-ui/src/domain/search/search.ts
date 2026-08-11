import { z } from "zod";
import { AccountProfile } from "@/domain/account/account";
import { Status, StatusListSchema } from "@/domain/status/status";

export type HashtagRef = Readonly<{
  id: string;
  name: string;
  url: string;
}>;

const HashtagRefSchema = z
  .object({
    id: z.string().optional().default(""),
    name: z.string().min(1),
    url: z.string(),
  })
  .transform(
    (value): HashtagRef => ({
      id: value.id || value.name,
      name: value.name,
      url: value.url,
    }),
  );

export type SearchResults = Readonly<{
  accounts: ReadonlyArray<AccountProfile>;
  statuses: ReadonlyArray<Status>;
  hashtags: ReadonlyArray<HashtagRef>;
}>;

export const SearchResults = {
  schema: z
    .object({
      accounts: z.array(AccountProfile.schema).optional().default([]),
      statuses: StatusListSchema.optional().default([]),
      hashtags: z.array(HashtagRefSchema).optional().default([]),
    })
    .transform(
      (value): SearchResults => ({
        accounts: value.accounts,
        statuses: value.statuses,
        hashtags: value.hashtags,
      }),
    ),
} as const;
