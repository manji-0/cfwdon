import type { AccountRef } from "@/domain/account/account";
import type { Status } from "@/domain/status/status";

type ConversationBody = Readonly<{
  id: string;
  accounts: ReadonlyArray<AccountRef>;
  lastStatus: Status | null;
}>;

export type ConversationRead = ConversationBody &
  Readonly<{
    kind: "Read";
  }>;

export type ConversationUnread = ConversationBody &
  Readonly<{
    kind: "Unread";
  }>;

export type Conversation = ConversationRead | ConversationUnread;

export const Conversation = {
  read: (fields: ConversationBody): ConversationRead => ({
    kind: "Read",
    ...fields,
  }),

  unread: (fields: ConversationBody): ConversationUnread => ({
    kind: "Unread",
    ...fields,
  }),

  isUnread: (conversation: Conversation) => conversation.kind === "Unread",

  markRead: (conversation: ConversationUnread): ConversationRead => ({
    kind: "Read",
    id: conversation.id,
    accounts: conversation.accounts,
    lastStatus: conversation.lastStatus,
  }),
} as const;
