export type Announcement = Readonly<{
  id: string;
  content: string;
  read: boolean;
  publishedAt: string | null;
}>;

export const Announcement = {
  isUnread: (item: Announcement): boolean => !item.read,
} as const;
