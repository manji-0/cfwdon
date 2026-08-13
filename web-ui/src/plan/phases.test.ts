import { describe, expect, it } from "vitest";
import { WebUiPhase, WebUiPhaseSummary } from "@/plan/phases";

describe("WebUiPhase", () => {
  it("documents every phase id", () => {
    for (const phase of Object.values(WebUiPhase)) {
      expect(WebUiPhaseSummary[phase]).toBeTruthy();
    }
  });

  it("uses unique phase ids", () => {
    const phases = Object.values(WebUiPhase);
    expect(new Set(phases).size).toBe(phases.length);
  });

  it("covers shell through collections", () => {
    expect(Object.keys(WebUiPhase)).toEqual([
      "shell",
      "timeline",
      "timelineMedia",
      "notificationsSearch",
      "settings",
      "streaming",
      "collections",
    ]);
  });
});
