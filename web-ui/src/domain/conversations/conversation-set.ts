import { Conversation } from "./conversation";

export type ConversationSet = ReadonlyArray<Conversation>;

export const ConversationSet = {
  empty: (): ConversationSet => [],

  replace: (items: ReadonlyArray<Conversation>): ConversationSet => items,

  appendPage: (set: ConversationSet, page: ReadonlyArray<Conversation>): ConversationSet => [
    ...set,
    ...page,
  ],

  upsert: (set: ConversationSet, conversation: Conversation): ConversationSet => [
    conversation,
    ...set.filter((item) => item.id !== conversation.id),
  ],

  unreadCount: (set: ConversationSet): number =>
    set.reduce((count, item) => count + (Conversation.isUnread(item) ? 1 : 0), 0),
} as const;
