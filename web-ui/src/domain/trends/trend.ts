export type TrendTagHistoryEntry = Readonly<{
  day: string;
  uses: string;
  accounts: string;
}>;

/** `/api/v1/trends/tags` response item. */
export type TrendTag = Readonly<{
  id: string;
  name: string;
  url: string;
  history: ReadonlyArray<TrendTagHistoryEntry>;
}>;
