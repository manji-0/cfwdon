import { type } from "arktype";
import type { AccountList } from "@/domain/lists/list";
import { ListRepliesPolicy } from "@/domain/lists/replies-policy";

export const parseAccountList = type({
  id: "string>0",
  title: "string",
  replies_policy: "string",
  exclusive: "boolean",
}).pipe(
  (value): AccountList => ({
    id: value.id,
    title: value.title,
    repliesPolicy: ListRepliesPolicy.fromApi(value.replies_policy),
    exclusive: value.exclusive,
  }),
);

export const parseAccountListCollection = type(parseAccountList, "[]");
