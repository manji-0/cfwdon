import type { AccountProfile } from "@/domain/account/account";
import type { Status } from "@/domain/status/status";

export type HashtagRef = Readonly<{
  id: string;
  name: string;
  url: string;
}>;

export type SearchResults = Readonly<{
  accounts: ReadonlyArray<AccountProfile>;
  statuses: ReadonlyArray<Status>;
  hashtags: ReadonlyArray<HashtagRef>;
}>;
