import { type ResultAsync } from "neverthrow";
import type { Conversation } from "@/domain/conversations/conversation";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonFetchJson, mastodonPostJson } from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import { parseConversation, parseConversationList } from "@/infrastructure/mastodon/parsers/conversations";

export type ConversationsQuery = Readonly<{
  maxId?: string;
  limit?: number;
}>;

export const fetchConversations = (
  query: ConversationsQuery = {},
): ResultAsync<ReadonlyArray<Conversation>, MastodonFetchError> => {
  const params = new URLSearchParams();
  params.set("limit", String(query.limit ?? 20));
  if (query.maxId) {
    params.set("max_id", query.maxId);
  }
  return mastodonFetchJson(`/api/v1/conversations?${params}`).andThen((raw) =>
    parseMastodon(parseConversationList, raw),
  );
};

export const markConversationRead = (
  conversationId: string,
): ResultAsync<Conversation, MastodonFetchError> =>
  mastodonPostJson(
    `/api/v1/conversations/${encodeURIComponent(conversationId)}/read`,
    {},
  ).andThen((raw) => parseMastodon(parseConversation, raw));
