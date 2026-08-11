import { z } from "zod";

const TrendTagHistoryEntrySchema = z
  .object({
    day: z.string(),
    uses: z.string(),
    accounts: z.string(),
  })
  .transform(
    (value): TrendTagHistoryEntry => ({
      day: value.day,
      uses: value.uses,
      accounts: value.accounts,
    }),
  );

export type TrendTagHistoryEntry = Readonly<{
  day: string;
  uses: string;
  accounts: string;
}>;

/** TODO(Phase 1): `/api/v1/trends/tags` response schema. */
export type TrendTag = Readonly<{
  id: string;
  name: string;
  url: string;
  history: ReadonlyArray<TrendTagHistoryEntry>;
}>;

export const TrendTagModel = {
  schema: z
    .object({
      id: z.string().min(1),
      name: z.string().min(1),
      url: z.string(),
      history: z.array(
        z.object({
          day: z.string(),
          uses: z.string(),
          accounts: z.string(),
        }),
      ),
    })
    .transform(
      (value): TrendTag => ({
        id: value.id,
        name: value.name,
        url: value.url,
        history: value.history.map((entry) => TrendTagHistoryEntrySchema.parse(entry)),
      }),
    ),

  listSchema: z.array(
    z
      .object({
        id: z.string().min(1),
        name: z.string().min(1),
        url: z.string(),
        history: z.array(
          z.object({
            day: z.string(),
            uses: z.string(),
            accounts: z.string(),
          }),
        ),
      })
      .transform(
        (value): TrendTag => ({
          id: value.id,
          name: value.name,
          url: value.url,
          history: value.history.map((entry) => ({
            day: entry.day,
            uses: entry.uses,
            accounts: entry.accounts,
          })),
        }),
      ),
  ),
} as const;
