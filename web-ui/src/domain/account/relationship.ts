import { assertNever } from "@/domain/never";

export type Relationship = Readonly<{
  id: string;
  following: boolean;
  followedBy: boolean;
  blocking: boolean;
  muting: boolean;
  requested: boolean;
  requestedBy: boolean;
  showingReblogs: boolean;
  notifying: boolean;
}>;

export const Relationship = {
  empty: (accountId: string): Relationship => ({
    id: accountId,
    following: false,
    followedBy: false,
    blocking: false,
    muting: false,
    requested: false,
    requestedBy: false,
    showingReblogs: true,
    notifying: false,
  }),

  followKind: (
    relationship: Relationship,
  ): "following" | "requested" | "none" => {
    if (relationship.following) {
      return "following";
    }
    if (relationship.requested) {
      return "requested";
    }
    return "none";
  },

  followLabel: (relationship: Relationship, locked: boolean): string => {
    const kind = Relationship.followKind(relationship);
    switch (kind) {
      case "following":
        return "フォロー中";
      case "requested":
        return "リクエスト中";
      case "none":
        if (locked) {
          return "フォローをリクエスト";
        }
        if (relationship.followedBy) {
          return "フォローバック";
        }
        return "フォロー";
      default:
        return assertNever(kind);
    }
  },
} as const;
