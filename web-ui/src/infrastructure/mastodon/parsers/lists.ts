import { type } from "arktype";
import type { AccountList } from "@/domain/lists/list";

export const parseAccountList = type({
  id: "string>0",
  title: "string",
  replies_policy: "string",
  exclusive: "boolean",
}).pipe(
  (value): AccountList => ({
    id: value.id,
    title: value.title,
    repliesPolicy: value.replies_policy,
    exclusive: value.exclusive,
  }),
);

export const parseAccountListCollection = type(parseAccountList, "[]");
