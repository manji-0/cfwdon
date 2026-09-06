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

export type SearchType = "all" | "accounts" | "statuses" | "hashtags";

export const SearchType = {
  values: ["all", "accounts", "statuses", "hashtags"] as const satisfies ReadonlyArray<SearchType>,

  fromParam: (value: string | null): SearchType => {
    switch (value) {
      case "accounts":
      case "statuses":
      case "hashtags":
        return value;
      default:
        return "all";
    }
  },

  label: (type: SearchType): string => {
    switch (type) {
      case "accounts":
        return "アカウント";
      case "statuses":
        return "投稿";
      case "hashtags":
        return "ハッシュタグ";
      default:
        return "すべて";
    }
  },

  /** Remote lookup for URLs and @user@domain / user@domain queries. */
  shouldResolve: (query: string): boolean => {
    const trimmed = query.trim();
    if (trimmed.startsWith("http://") || trimmed.startsWith("https://")) {
      return true;
    }
    return trimmed.includes("@");
  },
} as const;

export const emptySearchResults = (): SearchResults => ({
  accounts: [],
  statuses: [],
  hashtags: [],
});
