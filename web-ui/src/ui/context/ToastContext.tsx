import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";

export type ToastTone = "info" | "error";

type Toast = Readonly<{
  id: number;
  tone: ToastTone;
  message: string;
}>;

type ToastApi = Readonly<{
  info: (message: string) => void;
  error: (message: string) => void;
}>;

const TOAST_TIMEOUT_MS = 5000;
const MAX_TOASTS = 3;

const ToastContext = createContext<ToastApi | null>(null);

/**
 * Transient feedback for actions whose result would otherwise land off-screen
 * (e.g. a failed favourite far down the timeline). Error toasts are
 * role="alert" in an assertive region; info toasts are role="status".
 */
export const ToastProvider = ({ children }: Readonly<{ children: ReactNode }>) => {
  const [toasts, setToasts] = useState<ReadonlyArray<Toast>>([]);
  const nextId = useRef(0);
  const timers = useRef(new Map<number, ReturnType<typeof setTimeout>>());

  const dismiss = useCallback((id: number) => {
    const timer = timers.current.get(id);
    if (timer !== undefined) {
      clearTimeout(timer);
      timers.current.delete(id);
    }
    setToasts((current) => current.filter((toast) => toast.id !== id));
  }, []);

  const push = useCallback(
    (tone: ToastTone, message: string) => {
      const id = nextId.current++;
      setToasts((current) => [...current.filter((toast) => toast.message !== message), { id, tone, message }].slice(-MAX_TOASTS));
      timers.current.set(
        id,
        setTimeout(() => dismiss(id), TOAST_TIMEOUT_MS),
      );
    },
    [dismiss],
  );

  useEffect(() => {
    const pending = timers.current;
    return () => {
      for (const timer of pending.values()) {
        clearTimeout(timer);
      }
      pending.clear();
    };
  }, []);

  const value = useMemo<ToastApi>(
    () => ({
      info: (message) => push("info", message),
      error: (message) => push("error", message),
    }),
    [push],
  );

  const renderGroup = (tone: ToastTone) =>
    toasts
      .filter((toast) => toast.tone === tone)
      .map((toast) => (
        <div key={toast.id} className={`app-toast is-${toast.tone}`} role={toast.tone === "error" ? "alert" : "status"}>
          <span className="app-toast-message">{toast.message}</span>
          <button
            type="button"
            className="app-toast-dismiss"
            aria-label="閉じる"
            onClick={() => dismiss(toast.id)}
          >
            ×
          </button>
        </div>
      ));

  return (
    <ToastContext.Provider value={value}>
      {children}
      <div className="app-toasts">
        <div aria-live="assertive">{renderGroup("error")}</div>
        <div aria-live="polite">{renderGroup("info")}</div>
      </div>
    </ToastContext.Provider>
  );
};

export const useToast = (): ToastApi => {
  const value = useContext(ToastContext);
  if (!value) {
    throw new Error("useToast must be used within ToastProvider");
  }
  return value;
};
