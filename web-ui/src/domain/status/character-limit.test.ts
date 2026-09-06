import { describe, expect, it } from "vitest";
import { STATUS_MAX_CHARACTERS, StatusCharacters } from "@/domain/status/character-limit";

describe("StatusCharacters", () => {
  it("counts Unicode code points, not UTF-16 units", () => {
    expect(StatusCharacters.count("hi")).toBe(2);
    expect(StatusCharacters.count("あいう")).toBe(3);
    expect(StatusCharacters.count("🙂")).toBe(1);
  });

  it("reports remaining characters against the instance limit", () => {
    expect(StatusCharacters.remaining("")).toBe(STATUS_MAX_CHARACTERS);
    expect(StatusCharacters.remaining("a".repeat(10))).toBe(STATUS_MAX_CHARACTERS - 10);
    expect(StatusCharacters.isWithinLimit("a".repeat(STATUS_MAX_CHARACTERS))).toBe(true);
    expect(StatusCharacters.isWithinLimit("a".repeat(STATUS_MAX_CHARACTERS + 1))).toBe(false);
  });
});
