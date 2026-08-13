export type AccountRef = Readonly<{
  id: string;
  username: string;
  acct: string;
  displayName: string;
  avatar: string;
}>;

export type AccountProfile = Readonly<{
  id: string;
  username: string;
  acct: string;
  displayName: string;
  avatar: string;
  header: string;
  note: string;
  followersCount: number;
  followingCount: number;
  statusesCount: number;
  locked: boolean;
}>;
