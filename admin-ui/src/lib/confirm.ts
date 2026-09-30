import { writable } from "svelte/store";

export type ConfirmRequest = {
  title: string;
  message: string;
  confirmLabel: string;
  danger: boolean;
  resolve: (ok: boolean) => void;
};

/** The pending confirmation rendered by `ConfirmDialog` (mounted once in App). */
export const confirmRequest = writable<ConfirmRequest | null>(null);

/**
 * Ask before a destructive or hard-to-undo admin action.
 * Resolves `true` only when the admin presses the confirm button.
 */
export function confirmAction(
  message: string,
  options: { title?: string; confirmLabel?: string; danger?: boolean } = {},
): Promise<boolean> {
  return new Promise((resolve) => {
    confirmRequest.update((current) => {
      current?.resolve(false);
      return {
        title: options.title ?? "確認",
        message,
        confirmLabel: options.confirmLabel ?? "実行",
        danger: options.danger ?? true,
        resolve,
      };
    });
  });
}
