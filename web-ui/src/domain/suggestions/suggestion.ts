import type { AccountProfile } from "@/domain/account/account";

export type AccountSuggestion = Readonly<{
  source: string;
  account: AccountProfile;
}>;
