export type FeaturedTag = Readonly<{
  id: string;
  name: string;
  statusesCount: number;
  lastStatusAt: string | null;
}>;

export const FeaturedTag = {
  maxCount: 10,

  canAdd: (count: number): boolean => count < FeaturedTag.maxCount,
} as const;
