import type { ListRepliesPolicy } from "./replies-policy";

export type AccountList = Readonly<{
  id: string;
  title: string;
  repliesPolicy: ListRepliesPolicy;
  exclusive: boolean;
}>;
