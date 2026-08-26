import type { KeywordFilter } from "@/domain/filters/filter";
import { type } from "arktype";

const KeywordParser = type({
  id: "string>0",
  keyword: "string",
  whole_word: "boolean",
});

export const parseKeywordFilter = type({
  id: "string>0",
  title: "string",
  context: "string[]",
  "expires_at?": "string | null",
  filter_action: "string",
  "keywords?": KeywordParser.array(),
  "statuses?": "unknown[]",
}).pipe(
  (value): KeywordFilter => ({
    id: value.id,
    title: value.title,
    context: value.context,
    expiresAt: value.expires_at ?? null,
    filterAction: value.filter_action,
    keywords: (value.keywords ?? []).map((keyword) => ({
      id: keyword.id,
      keyword: keyword.keyword,
      wholeWord: keyword.whole_word,
    })),
  }),
);

export const parseKeywordFilterList = type(parseKeywordFilter, "[]");
