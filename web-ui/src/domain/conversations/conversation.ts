import type { AccountRef } from "@/domain/account/account";
import type { Status } from "@/domain/status/status";

export type Conversation = Readonly<{
  id: string;
  unread: boolean;
  accounts: ReadonlyArray<AccountRef>;
  lastStatus: Status | null;
}>;
