import type { FeaturedTag } from "@/domain/tags/featured-tag";
import { type } from "arktype";

export const parseFeaturedTag = type({
  id: "string>0",
  name: "string>0",
  "url?": "string",
  "statuses_count?": "number | string",
  "last_status_at?": "string | null",
}).pipe((value): FeaturedTag => {
  const count =
    typeof value.statuses_count === "string"
      ? Number.parseInt(value.statuses_count, 10)
      : (value.statuses_count ?? 0);
  return {
    id: value.id,
    name: value.name,
    statusesCount: Number.isFinite(count) ? count : 0,
    lastStatusAt: value.last_status_at ?? null,
  };
});

export const parseFeaturedTagList = type(parseFeaturedTag, "[]");

export const parseFeaturedTagSuggestion = type({
  name: "string>0",
  "id?": "string",
  "url?": "string",
  "history?": "unknown[]",
  "following?": "boolean | null",
}).pipe((value): FeaturedTag => ({
  id: value.id ?? value.name,
  name: value.name,
  statusesCount: 0,
  lastStatusAt: null,
}));

export const parseFeaturedTagSuggestionList = type(parseFeaturedTagSuggestion, "[]");
