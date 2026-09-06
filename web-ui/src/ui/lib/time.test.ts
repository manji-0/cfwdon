import { afterEach, describe, expect, it, vi } from "vitest";
import { formatExpiry } from "@/ui/lib/time";

describe("formatExpiry", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("labels missing or invalid dates as open-ended", () => {
    expect(formatExpiry(null)).toBe("期限なし");
    expect(formatExpiry("not-a-date")).toBe("期限なし");
  });

  it("labels past dates as expired and future dates as remaining", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-09-07T00:00:00.000Z"));
    expect(formatExpiry("2026-09-06T23:59:59.000Z")).toBe("期限切れ");
    expect(formatExpiry("2026-09-07T00:00:30.000Z")).toMatch(/まで$/);
  });
});
