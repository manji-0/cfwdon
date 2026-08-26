import type { TrendTagHistoryEntry } from "@/domain/trends/trend";

export type FollowedTag = Readonly<{
  id: string;
  name: string;
  url: string;
  following: boolean;
  history: ReadonlyArray<TrendTagHistoryEntry>;
}>;
