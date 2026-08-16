import { type } from "arktype";
import { Conversation } from "@/domain/conversations/conversation";
import { parseAccountRef } from "@/infrastructure/mastodon/parsers/account";
import { parseStatus } from "@/infrastructure/mastodon/parsers/status";

export const parseConversation = type({
  id: "string>0",
  unread: "boolean",
  accounts: parseAccountRef.array(),
  "last_status?": parseStatus.or("null"),
}).pipe((value): Conversation => {
  const fields = {
    id: value.id,
    accounts: value.accounts,
    lastStatus: value.last_status ?? null,
  };
  return value.unread ? Conversation.unread(fields) : Conversation.read(fields);
});

export const parseConversationList = type(parseConversation, "[]");
