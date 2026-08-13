export type AccountCredentials = Readonly<{
  id: string;
  displayName: string;
  note: string;
  avatar: string;
  header: string;
  username: string;
  acct: string;
  source: Readonly<{
    note: string;
    privacy: string;
    sensitive: boolean;
    language: string | null;
    quotePolicy: string;
  }>;
}>;
