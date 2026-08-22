import type { TrendLink } from "@/domain/trends/link";
import { type } from "arktype";

export const parseTrendLink = type({
  url: "string",
  "title?": "string",
  "description?": "string",
  "image?": "string | null",
}).pipe(
  (value): TrendLink => ({
    url: value.url,
    title: value.title ?? value.url,
    description: value.description ?? "",
    image: value.image ?? null,
  }),
);

export const parseTrendLinkList = type(parseTrendLink, "[]");
