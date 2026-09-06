import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";

export type ConfirmOptions = Readonly<{
  title?: string;
  confirmLabel?: string;
  danger?: boolean;
}>;

export type PromptOptions = Readonly<{
  title?: string;
  confirmLabel?: string;
  defaultValue?: string;
}>;

export type AlertOptions = Readonly<{
  title?: string;
  confirmLabel?: string;
}>;

type ConfirmApi = Readonly<{
  confirm: (message: string, options?: ConfirmOptions) => Promise<boolean>;
  prompt: (message: string, options?: PromptOptions) => Promise<string | null>;
  alert: (message: string, options?: AlertOptions) => Promise<void>;
}>;

type ConfirmDialogState = Readonly<{
  kind: "confirm" | "prompt" | "alert";
  title: string;
  message: string;
  confirmLabel: string;
  danger: boolean;
  inputValue: string;
}>;

type Resolver = Readonly<{
  resolveConfirm: (value: boolean) => void;
  resolvePrompt: (value: string | null) => void;
  resolveAlert: () => void;
}>;

const ConfirmContext = createContext<ConfirmApi | null>(null);

export const ConfirmProvider = ({ children }: Readonly<{ children: ReactNode }>) => {
  const [dialog, setDialog] = useState<(ConfirmDialogState & Resolver) | null>(null);
  const [inputValue, setInputValue] = useState("");

  const close = useCallback(() => {
    setDialog(null);
    setInputValue("");
  }, []);

  const confirm = useCallback((message: string, options: ConfirmOptions = {}) => {
    return new Promise<boolean>((resolve) => {
      setInputValue("");
      setDialog({
        kind: "confirm",
        title: options.title ?? "確認",
        message,
        confirmLabel: options.confirmLabel ?? "OK",
        danger: options.danger ?? false,
        inputValue: "",
        resolveConfirm: resolve,
        resolvePrompt: () => undefined,
        resolveAlert: () => undefined,
      });
    });
  }, []);

  const prompt = useCallback((message: string, options: PromptOptions = {}) => {
    return new Promise<string | null>((resolve) => {
      setInputValue(options.defaultValue ?? "");
      setDialog({
        kind: "prompt",
        title: options.title ?? "入力",
        message,
        confirmLabel: options.confirmLabel ?? "OK",
        danger: false,
        inputValue: options.defaultValue ?? "",
        resolveConfirm: () => undefined,
        resolvePrompt: resolve,
        resolveAlert: () => undefined,
      });
    });
  }, []);

  const alert = useCallback((message: string, options: AlertOptions = {}) => {
    return new Promise<void>((resolve) => {
      setInputValue("");
      setDialog({
        kind: "alert",
        title: options.title ?? "お知らせ",
        message,
        confirmLabel: options.confirmLabel ?? "OK",
        danger: false,
        inputValue: "",
        resolveConfirm: () => undefined,
        resolvePrompt: () => undefined,
        resolveAlert: resolve,
      });
    });
  }, []);

  const value = useMemo(() => ({ confirm, prompt, alert }), [alert, confirm, prompt]);

  const handleCancel = () => {
    if (!dialog) {
      return;
    }
    if (dialog.kind === "confirm") {
      dialog.resolveConfirm(false);
    } else if (dialog.kind === "prompt") {
      dialog.resolvePrompt(null);
    } else {
      dialog.resolveAlert();
    }
    close();
  };

  const handleOk = () => {
    if (!dialog) {
      return;
    }
    if (dialog.kind === "confirm") {
      dialog.resolveConfirm(true);
    } else if (dialog.kind === "prompt") {
      dialog.resolvePrompt(inputValue);
    } else {
      dialog.resolveAlert();
    }
    close();
  };

  useEffect(() => {
    if (!dialog) {
      return undefined;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") {
        return;
      }
      event.preventDefault();
      event.stopImmediatePropagation();
      handleCancel();
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [dialog]);

  return (
    <ConfirmContext.Provider value={value}>
      {children}
      {dialog ? (
        <div className="shortcut-overlay" data-app-overlay="true" role="presentation" onClick={handleCancel}>
          <section
            className="shortcut-dialog app-card"
            role="dialog"
            aria-modal="true"
            aria-labelledby="confirm-dialog-title"
            onClick={(event) => event.stopPropagation()}
          >
            <header className="shortcut-dialog-header">
              <h2 id="confirm-dialog-title">{dialog.title}</h2>
            </header>
            <p className="confirm-dialog-message">{dialog.message}</p>
            {dialog.kind === "prompt" ? (
              <label className="settings-field">
                <span>内容</span>
                <input
                  autoFocus
                  value={inputValue}
                  onChange={(event) => setInputValue(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      event.preventDefault();
                      handleOk();
                    }
                    if (event.key === "Escape") {
                      event.preventDefault();
                      handleCancel();
                    }
                  }}
                />
              </label>
            ) : null}
            <div className="confirm-dialog-actions">
              {dialog.kind !== "alert" ? (
                <button type="button" className="app-button app-button-secondary" onClick={handleCancel}>
                  キャンセル
                </button>
              ) : null}
              <button
                type="button"
                className={dialog.danger ? "app-button app-button-danger" : "app-button"}
                onClick={handleOk}
                autoFocus={dialog.kind !== "prompt"}
              >
                {dialog.confirmLabel}
              </button>
            </div>
          </section>
        </div>
      ) : null}
    </ConfirmContext.Provider>
  );
};

export const useConfirm = (): ConfirmApi => {
  const value = useContext(ConfirmContext);
  if (!value) {
    throw new Error("useConfirm must be used within ConfirmProvider");
  }
  return value;
};
