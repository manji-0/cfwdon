export type ListRepliesPolicy = "followed" | "list" | "none";

export const ListRepliesPolicy = {
  values: ["followed", "list", "none"] as const satisfies ReadonlyArray<ListRepliesPolicy>,

  defaultValue: (): ListRepliesPolicy => "list",

  fromApi: (value: string): ListRepliesPolicy => {
    switch (value.trim().toLowerCase()) {
      case "followed":
      case "list":
      case "none":
        return value.trim().toLowerCase() as ListRepliesPolicy;
      default:
        return ListRepliesPolicy.defaultValue();
    }
  },

  label: (policy: ListRepliesPolicy): string => {
    switch (policy) {
      case "followed":
        return "フォロー中のみ返信を表示";
      case "list":
        return "リスト内の返信を表示";
      case "none":
        return "返信を表示しない";
    }
  },
} as const;
