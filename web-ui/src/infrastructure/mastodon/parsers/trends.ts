import { type } from "arktype";
import type { TrendTag, TrendTagHistoryEntry } from "@/domain/trends/trend";

const TrendTagHistoryEntryParser = type({
  day: "string",
  uses: "string",
  accounts: "string",
}).pipe(
  (value): TrendTagHistoryEntry => ({
    day: value.day,
    uses: value.uses,
    accounts: value.accounts,
  }),
);

const TrendTagParser = type({
  id: "string>0",
  name: "string>0",
  url: "string",
  history: TrendTagHistoryEntryParser.array(),
}).pipe(
  (value): TrendTag => ({
    id: value.id,
    name: value.name,
    url: value.url,
    history: value.history,
  }),
);

export const parseTrendTag = TrendTagParser;
export const parseTrendTagList = TrendTagParser.array();
