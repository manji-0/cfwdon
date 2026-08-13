import type { Conversation } from "@/domain/conversations/conversation";

export const countUnreadConversations = (
  conversations: ReadonlyArray<Pick<Conversation, "unread">>,
): number => conversations.reduce((total, conversation) => total + (conversation.unread ? 1 : 0), 0);
