import type { StatusSource } from "@/domain/status/source";
import { type } from "arktype";

export const parseStatusSource = type({
  id: "string>0",
  text: "string",
  spoiler_text: "string",
}).pipe(
  (value): StatusSource => ({
    id: value.id,
    text: value.text,
    spoilerText: value.spoiler_text,
  }),
);

export const parseStatusEditList = type(
  {
    content: "string",
    "spoiler_text?": "string",
    created_at: "string",
  },
  "[]",
).pipe((value) =>
  value.map((entry) => ({
    content: entry.content,
    spoilerText: entry.spoiler_text ?? "",
    createdAt: entry.created_at,
  })),
);

export const parseStatusTranslation = type({
  content: "string",
  "spoiler_text?": "string",
  "detected_source_language?": "string",
  "language?": "string",
  "provider?": "string",
}).pipe((value) => ({
  content: value.content,
  spoilerText: value.spoiler_text ?? "",
  detectedSourceLanguage: value.detected_source_language ?? "",
  language: value.language ?? "",
  provider: value.provider ?? "",
}));
