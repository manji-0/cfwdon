import { describe, expect, it } from "vitest";
import { isSubmitShortcut } from "@/ui/lib/keyboard";

describe("keyboard", () => {
  it("detects modifier+Enter as submit shortcut", () => {
    expect(isSubmitShortcut({ key: "Enter", metaKey: true, ctrlKey: false, shiftKey: false, altKey: false })).toBe(
      true,
    );
    expect(isSubmitShortcut({ key: "Enter", metaKey: false, ctrlKey: true, shiftKey: false, altKey: false })).toBe(
      true,
    );
    expect(isSubmitShortcut({ key: "Enter", metaKey: false, ctrlKey: false, shiftKey: false, altKey: false })).toBe(
      false,
    );
    expect(isSubmitShortcut({ key: "Enter", metaKey: true, ctrlKey: false, shiftKey: true, altKey: false })).toBe(
      false,
    );
  });
});
