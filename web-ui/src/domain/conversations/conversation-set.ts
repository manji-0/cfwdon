import { Conversation, type Conversation as ConversationState } from "./conversation";

export type ConversationSet = ReadonlyArray<ConversationState>;

export const ConversationSet = {
  empty: (): ConversationSet => [],

  replace: (items: ReadonlyArray<ConversationState>): ConversationSet => items,

  appendPage: (set: ConversationSet, page: ReadonlyArray<ConversationState>): ConversationSet => [
    ...set,
    ...page,
  ],

  upsert: (set: ConversationSet, conversation: ConversationState): ConversationSet => [
    conversation,
    ...set.filter((item) => item.id !== conversation.id),
  ],

  unreadCount: (set: ConversationSet): number => set.filter(Conversation.isUnread).length,
} as const;
