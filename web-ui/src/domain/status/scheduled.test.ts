import { describe, expect, it } from "vitest";
import { ScheduledAt } from "@/domain/status/scheduled";

describe("ScheduledAt", () => {
  it("rejects timestamps inside the five-minute minimum", () => {
    const now = Date.parse("2026-09-07T00:00:00.000Z");
    expect(ScheduledAt.isFarEnough("2026-09-07T00:04:00.000Z", now)).toBe(false);
    expect(ScheduledAt.isFarEnough("2026-09-07T00:06:00.000Z", now)).toBe(true);
  });

  it("round-trips a local datetime into RFC 3339", () => {
    const rfc = ScheduledAt.toRfc3339("2026-09-07T12:30");
    expect(rfc).toMatch(/^\d{4}-\d{2}-\d{2}T/);
    expect(ScheduledAt.toLocalValue(rfc ?? "")).toMatch(/T\d{2}:\d{2}$/);
  });
});
