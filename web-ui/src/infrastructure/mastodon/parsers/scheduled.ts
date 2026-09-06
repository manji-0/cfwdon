import type { ScheduledStatus } from "@/domain/status/scheduled";
import { type } from "arktype";

const ScheduledStatusParser = type({
  id: "string>0",
  scheduled_at: "string",
  params: {
    "text?": "string | null",
    "spoiler_text?": "string | null",
    "visibility?": "string | null",
    "sensitive?": "boolean | null",
  },
}).pipe(
  (value): ScheduledStatus => ({
    id: value.id,
    scheduledAt: value.scheduled_at,
    text: value.params.text ?? "",
    spoilerText: value.params.spoiler_text ?? "",
    visibility: value.params.visibility ?? "public",
    sensitive: value.params.sensitive ?? false,
  }),
);

export const parseScheduledStatus = ScheduledStatusParser;
export const parseScheduledStatusList = type(ScheduledStatusParser, "[]");
