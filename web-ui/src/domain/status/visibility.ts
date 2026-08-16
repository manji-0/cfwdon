import { assertNever } from "@/domain/never";

export type Visibility =
  | Readonly<{ kind: "Public" }>
  | Readonly<{ kind: "Unlisted" }>
  | Readonly<{ kind: "Private" }>
  | Readonly<{ kind: "Direct" }>;

export const Visibility = {
  public: (): Visibility => ({ kind: "Public" }),
  unlisted: (): Visibility => ({ kind: "Unlisted" }),
  private: (): Visibility => ({ kind: "Private" }),
  direct: (): Visibility => ({ kind: "Direct" }),

  fromApi: (value: string): Visibility => {
    switch (value) {
      case "public":
        return Visibility.public();
      case "unlisted":
        return Visibility.unlisted();
      case "private":
        return Visibility.private();
      case "direct":
        return Visibility.direct();
      default:
        // Unknown wire values are treated as public. ArkType rejects them at the parser.
        return Visibility.public();
    }
  },

  toApi: (visibility: Visibility): string => {
    switch (visibility.kind) {
      case "Public":
        return "public";
      case "Unlisted":
        return "unlisted";
      case "Private":
        return "private";
      case "Direct":
        return "direct";
      default:
        return assertNever(visibility);
    }
  },

  label: (visibility: Visibility): string => {
    switch (visibility.kind) {
      case "Public":
        return "公開";
      case "Unlisted":
        return "未収載";
      case "Private":
        return "フォロワーのみ";
      case "Direct":
        return "ダイレクト";
      default:
        return assertNever(visibility);
    }
  },
} as const;
