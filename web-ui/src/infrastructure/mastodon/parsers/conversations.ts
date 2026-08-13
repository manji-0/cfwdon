import { type } from "arktype";
import type { Conversation } from "@/domain/conversations/conversation";
import { parseAccountRef } from "@/infrastructure/mastodon/parsers/account";
import { parseStatus } from "@/infrastructure/mastodon/parsers/status";

export const parseConversation = type({
  id: "string>0",
  unread: "boolean",
  accounts: parseAccountRef.array(),
  "last_status?": parseStatus.or("null"),
}).pipe(
  (value): Conversation => ({
    id: value.id,
    unread: value.unread,
    accounts: value.accounts,
    lastStatus: value.last_status ?? null,
  }),
);

export const parseConversationList = type(parseConversation, "[]");
