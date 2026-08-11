import { type } from "arktype";
import type { HashtagRef, SearchResults } from "@/domain/search/search";
import { parseAccountProfile } from "@/infrastructure/mastodon/parsers/account";
import { parseStatusList } from "@/infrastructure/mastodon/parsers/status";

const HashtagRefParser = type({
  "id?": "string",
  name: "string>0",
  url: "string",
}).pipe(
  (value): HashtagRef => ({
    id: value.id || value.name,
    name: value.name,
    url: value.url,
  }),
);

export const parseSearchResults = type({
  "accounts?": parseAccountProfile.array(),
  "statuses?": parseStatusList,
  "hashtags?": HashtagRefParser.array(),
}).pipe(
  (value): SearchResults => ({
    accounts: value.accounts ?? [],
    statuses: value.statuses ?? [],
    hashtags: value.hashtags ?? [],
  }),
);
