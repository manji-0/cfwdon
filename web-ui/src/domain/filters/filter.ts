export type FilterContext =
  | "home"
  | "notifications"
  | "public"
  | "thread"
  | "account";

export type FilterAction = "warn" | "hide" | "blur";

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

  fromApiList: (values: ReadonlyArray<string>): ReadonlyArray<FilterContext> =>
    FilterContext.values.filter((context) => values.includes(context)),

  toggle: (
    contexts: ReadonlyArray<FilterContext>,
    context: FilterContext,
  ): ReadonlyArray<FilterContext> =>
    contexts.includes(context)
      ? contexts.filter((item) => item !== context)
      : [...contexts, context],
} as const;

export const FilterAction = {
  values: ["warn", "hide", "blur"] as const satisfies ReadonlyArray<FilterAction>,

  defaultValue: (): FilterAction => "warn",

  fromApi: (value: string): FilterAction => {
    switch (value) {
      case "hide":
      case "blur":
        return value;
      default:
        return "warn";
    }
  },

  label: (action: string): string => {
    switch (action) {
      case "hide":
        return "隠す";
      case "blur":
        return "ぼかす";
      default:
        return "警告";
    }
  },
} as const;
