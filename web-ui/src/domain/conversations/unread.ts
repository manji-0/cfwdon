import { ConversationSet } from "./conversation-set";
import type { Conversation } from "./conversation";

export const countUnreadConversations = (conversations: ReadonlyArray<Conversation>): number =>
  ConversationSet.unreadCount(conversations);
