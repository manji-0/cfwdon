import type { CustomEmoji } from "@/domain/emoji/custom-emoji";
import { type } from "arktype";

export const parseCustomEmoji = type({
  shortcode: "string>0",
  url: "string",
  "static_url?": "string",
  "visible_in_picker?": "boolean",
  "category?": "string | null",
}).pipe(
  (value): CustomEmoji => ({
    shortcode: value.shortcode,
    url: value.url,
    staticUrl: value.static_url ?? value.url,
    visibleInPicker: value.visible_in_picker ?? true,
    category: value.category ?? null,
  }),
);

export const parseCustomEmojiList = type(parseCustomEmoji, "[]");
