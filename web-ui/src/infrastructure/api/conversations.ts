import { errAsync, okAsync, type ResultAsync } from "neverthrow";
import type { Conversation } from "@/domain/conversations/conversation";
import type { MastodonFetchError } from "@/domain/errors/http-error";
import type { PageQuery } from "@/domain/pagination";
import {
  mastodonDeleteJson,
  mastodonFetchJson,
  mastodonPostJson,
  pageParams,
} from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import { parseConversation, parseConversationList } from "@/infrastructure/mastodon/parsers/conversations";

export const fetchConversations = (
  query: PageQuery = {},
): ResultAsync<ReadonlyArray<Conversation>, MastodonFetchError> => {
  const params = pageParams(query);
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

export const deleteConversation = (
  conversationId: string,
): ResultAsync<null, MastodonFetchError> =>
  mastodonDeleteJson(`/api/v1/conversations/${encodeURIComponent(conversationId)}`).map(() => null);

export const findConversationById = (
  conversationId: string,
): ResultAsync<Conversation, MastodonFetchError> =>
  fetchConversations({ limit: 80 }).andThen((conversations) => {
    const found = conversations.find((item) => item.id === conversationId);
    if (!found) {
      return errAsync({
        kind: "HttpStatus",
        status: 404,
        body: "会話が見つかりません",
      } as const);
    }
    return okAsync(found);
  });

export const findConversationByStatusId = (
  statusId: string,
): ResultAsync<Conversation, MastodonFetchError> =>
  fetchConversations({ limit: 80 }).andThen((conversations) => {
    const found = conversations.find((item) => item.lastStatus?.id === statusId);
    if (!found) {
      return errAsync({
        kind: "HttpStatus",
        status: 404,
        body: "会話が見つかりません",
      } as const);
    }
    return okAsync(found);
  });
