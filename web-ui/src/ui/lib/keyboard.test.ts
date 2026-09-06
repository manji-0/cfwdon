/** @vitest-environment happy-dom */
import { describe, expect, it } from "vitest";
import { isHelpShortcut, isOverlayOpen, isSubmitShortcut, isTypingTarget } from "@/ui/lib/keyboard";

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

  it("detects ? as help shortcut without modifiers", () => {
    expect(isHelpShortcut({ key: "?", metaKey: false, ctrlKey: false, shiftKey: true, altKey: false })).toBe(
      true,
    );
    expect(isHelpShortcut({ key: "?", metaKey: true, ctrlKey: false, shiftKey: true, altKey: false })).toBe(
      false,
    );
    expect(isHelpShortcut({ key: "/", metaKey: false, ctrlKey: false, shiftKey: false, altKey: false })).toBe(
      false,
    );
  });

  it("treats inputs as typing targets and overlays as blocking", () => {
    const input = document.createElement("input");
    expect(isTypingTarget(input)).toBe(true);
    expect(isTypingTarget(document.createElement("div"))).toBe(false);
    expect(isOverlayOpen()).toBe(false);
    const overlay = document.createElement("div");
    overlay.setAttribute("data-app-overlay", "true");
    document.body.append(overlay);
    expect(isOverlayOpen()).toBe(true);
    overlay.remove();
  });
});
