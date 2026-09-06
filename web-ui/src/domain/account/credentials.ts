import type { ProfileField } from "@/domain/account/profile-field";

export type AccountCredentials = Readonly<{
  id: string;
  displayName: string;
  note: string;
  avatar: string;
  header: string;
  username: string;
  acct: string;
  locked: boolean;
  bot: boolean;
  discoverable: boolean;
  fields: ReadonlyArray<ProfileField>;
  source: Readonly<{
    note: string;
    privacy: string;
    sensitive: boolean;
    language: string | null;
    quotePolicy: string;
  }>;
}>;
