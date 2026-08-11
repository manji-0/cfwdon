import { useEffect, useRef } from "react";
import { isTypingTarget } from "@/ui/lib/keyboard";

export type KeyboardShortcut = Readonly<{
  key: string;
  handler: () => void;
  when?: () => boolean;
  allowInInput?: boolean;
}>;

export const useKeyboardShortcuts = (shortcuts: ReadonlyArray<KeyboardShortcut>) => {
  const shortcutsRef = useRef(shortcuts);
  shortcutsRef.current = shortcuts;

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const typing = isTypingTarget(event.target);
      for (const shortcut of shortcutsRef.current) {
        if (shortcut.when && !shortcut.when()) {
          continue;
        }
        if (typing && !shortcut.allowInInput) {
          continue;
        }
        if (event.key !== shortcut.key) {
          continue;
        }
        if (event.metaKey || event.ctrlKey || event.altKey) {
          continue;
        }
        event.preventDefault();
        shortcut.handler();
        return;
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
};
