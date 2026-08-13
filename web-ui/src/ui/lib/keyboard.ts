export type KeyboardEventLike = Readonly<
  Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "shiftKey" | "altKey">
>;

export const isMacPlatform = (): boolean =>
  typeof navigator !== "undefined" &&
  /Mac|iPhone|iPad|iPod/.test(navigator.platform);

export const modKeyLabel = (): string => (isMacPlatform() ? "⌘" : "Ctrl");

export const isSubmitShortcut = (event: KeyboardEventLike): boolean =>
  event.key === "Enter" && (event.metaKey || event.ctrlKey) && !event.shiftKey && !event.altKey;

export const isTypingTarget = (target: EventTarget | null): boolean => {
  if (!(target instanceof HTMLElement)) {
    return false;
  }
  const tag = target.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") {
    return true;
  }
  return target.isContentEditable;
};

export const isHelpShortcut = (event: KeyboardEventLike): boolean =>
  event.key === "?" && !event.metaKey && !event.ctrlKey && !event.altKey;
