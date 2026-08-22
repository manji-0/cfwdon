export type FilterContext =
  | "home"
  | "notifications"
  | "public"
  | "thread"
  | "account";

export type KeywordFilter = Readonly<{
  id: string;
  title: string;
  context: ReadonlyArray<string>;
  expiresAt: string | null;
  filterAction: string;
  keywords: ReadonlyArray<Readonly<{ id: string; keyword: string; wholeWord: boolean }>>;
}>;

export const FilterContext = {
  values: ["home", "notifications", "public", "thread", "account"] as const satisfies ReadonlyArray<FilterContext>,

  label: (context: string): string => {
    switch (context) {
      case "home":
        return "ホーム";
      case "notifications":
        return "通知";
      case "public":
        return "公開TL";
      case "thread":
        return "スレッド";
      case "account":
        return "プロフィール";
      default:
        return context;
    }
  },
} as const;
