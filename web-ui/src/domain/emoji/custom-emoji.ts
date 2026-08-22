export type CustomEmoji = Readonly<{
  shortcode: string;
  url: string;
  staticUrl: string;
  visibleInPicker: boolean;
  category: string | null;
}>;
