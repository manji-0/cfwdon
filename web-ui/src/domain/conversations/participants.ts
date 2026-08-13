import type { AccountRef } from "@/domain/account/account";

export const conversationTitle = (accounts: ReadonlyArray<AccountRef>): string => {
  if (accounts.length === 0) {
    return "会話";
  }
  return accounts.map((account) => account.displayName || account.username).join("、");
};

export const conversationAcctsLabel = (accounts: ReadonlyArray<AccountRef>): string =>
  accounts.map((account) => `@${account.acct}`).join(" ");
