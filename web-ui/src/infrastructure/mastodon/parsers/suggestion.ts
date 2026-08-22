import type { AccountSuggestion } from "@/domain/suggestions/suggestion";
import { type } from "arktype";
import { parseAccountProfile } from "@/infrastructure/mastodon/parsers/account";

export const parseAccountSuggestion = type({
  source: "string",
  account: parseAccountProfile,
}).pipe(
  (value): AccountSuggestion => ({
    source: value.source,
    account: value.account,
  }),
);

export const parseAccountSuggestionList = type(parseAccountSuggestion, "[]");
