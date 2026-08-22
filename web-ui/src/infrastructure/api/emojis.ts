import { type ResultAsync } from "neverthrow";
import type { CustomEmoji } from "@/domain/emoji/custom-emoji";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonFetchJson } from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import { parseCustomEmojiList } from "@/infrastructure/mastodon/parsers/emoji";

export const fetchCustomEmojis = (): ResultAsync<
  ReadonlyArray<CustomEmoji>,
  MastodonFetchError
> =>
  mastodonFetchJson("/api/v1/custom_emojis").andThen((raw) =>
    parseMastodon(parseCustomEmojiList, raw),
  );
