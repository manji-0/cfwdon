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

export type FilterExpirePreset = "never" | "keep" | "30m" | "1h" | "6h" | "12h" | "1d" | "7d";

export const FilterExpire = {
  createPresets: ["never", "30m", "1h", "6h", "12h", "1d", "7d"] as const satisfies ReadonlyArray<
    Exclude<FilterExpirePreset, "keep">
  >,
  editPresets: ["keep", "30m", "1h", "6h", "12h", "1d", "7d"] as const satisfies ReadonlyArray<
    Exclude<FilterExpirePreset, "never">
  >,

  seconds: (preset: FilterExpirePreset): number | undefined => {
    switch (preset) {
      case "never":
      case "keep":
        return undefined;
      case "30m":
        return 30 * 60;
      case "1h":
        return 60 * 60;
      case "6h":
        return 6 * 60 * 60;
      case "12h":
        return 12 * 60 * 60;
      case "1d":
        return 24 * 60 * 60;
      case "7d":
        return 7 * 24 * 60 * 60;
    }
  },

  label: (preset: FilterExpirePreset): string => {
    switch (preset) {
      case "never":
        return "期限なし";
      case "keep":
        return "期限を変更しない";
      case "30m":
        return "30分";
      case "1h":
        return "1時間";
      case "6h":
        return "6時間";
      case "12h":
        return "12時間";
      case "1d":
        return "1日";
      case "7d":
        return "7日";
    }
  },
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
