import { useEffect, type RefObject } from "react";

const TABBABLE = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled]):not([type=hidden])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "audio[controls]",
  "video[controls]",
  '[tabindex]:not([tabindex="-1"])',
].join(",");

const tabbablesIn = (container: HTMLElement): HTMLElement[] =>
  [...container.querySelectorAll<HTMLElement>(TABBABLE)].filter((element) => !element.hidden);

/**
 * Keeps Tab / Shift+Tab inside `ref` while `active`, moves focus into it on open,
 * and restores focus to the previously focused element on close.
 *
 * Only Tab wrapping is enforced (no focus pull-back), so a second dialog rendered
 * elsewhere in the DOM (e.g. a confirm over the compose sheet) can take focus.
 * `initialFocus` picks the element to focus when nothing inside is focused yet.
 */
export const useFocusTrap = (
  ref: RefObject<HTMLElement | null>,
  active: boolean,
  initialFocus?: () => HTMLElement | null | undefined,
): void => {
  useEffect(() => {
    const container = ref.current;
    if (!active || !container) {
      return undefined;
    }
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;

    if (!container.contains(document.activeElement)) {
      const target = initialFocus?.() ?? tabbablesIn(container)[0] ?? container;
      target.focus();
    }

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Tab") {
        return;
      }
      const tabbables = tabbablesIn(container);
      if (tabbables.length === 0) {
        event.preventDefault();
        container.focus();
        return;
      }
      const first = tabbables[0]!;
      const last = tabbables[tabbables.length - 1]!;
      const current = document.activeElement;
      if (event.shiftKey && (current === first || current === container)) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && current === last) {
        event.preventDefault();
        first.focus();
      }
    };
    container.addEventListener("keydown", onKeyDown);
    return () => {
      container.removeEventListener("keydown", onKeyDown);
      if (previous?.isConnected) {
        previous.focus();
      }
    };
    // initialFocus is read once on activation; callers pass inline closures.
  }, [active, ref]);
};
