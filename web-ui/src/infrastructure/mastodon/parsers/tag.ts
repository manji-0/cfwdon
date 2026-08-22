import type { FollowedTag } from "@/domain/tags/followed-tag";
import { type } from "arktype";

const TagHistoryParser = type({
  day: "string",
  uses: "string",
  accounts: "string",
});

export const parseFollowedTag = type({
  id: "string>0",
  name: "string>0",
  url: "string",
  "following?": "boolean | null",
  "history?": TagHistoryParser.array(),
}).pipe(
  (value): FollowedTag => ({
    id: value.id,
    name: value.name,
    url: value.url,
    following: value.following ?? true,
    history: value.history ?? [],
  }),
);

export const parseFollowedTagList = type(parseFollowedTag, "[]");
