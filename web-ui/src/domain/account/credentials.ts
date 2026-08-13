export type AccountCredentials = Readonly<{
  id: string;
  displayName: string;
  note: string;
  avatar: string;
  username: string;
  acct: string;
  source: Readonly<{
    privacy: string;
    sensitive: boolean;
    language: string | null;
    quotePolicy: string;
  }>;
}>;
