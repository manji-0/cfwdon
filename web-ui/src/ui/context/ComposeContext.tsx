import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import type { QuotedStatusPreview } from "@/domain/status/quote";
import type { Status } from "@/domain/status/status";
import { useKeyboardShortcuts } from "@/ui/hooks/useKeyboardShortcuts";
import { isOverlayOpen } from "@/ui/lib/keyboard";

export type ComposeIntent =
  | { kind: "Closed" }
  | { kind: "New" }
  | { kind: "Quote"; quotedStatusId: string; quotedPreview: QuotedStatusPreview }
  | { kind: "Edit"; statusId: string };

export type ComposeContextValue = Readonly<{
  intent: ComposeIntent;
  lastPublished: Status | null;
  lastPublishToken: number;
  openNew: () => void;
  openQuote: (quotedPreview: QuotedStatusPreview) => void;
  openEdit: (statusId: string) => void;
  close: () => void;
  markPublished: (status: Status) => void;
}>;

const ComposeContext = createContext<ComposeContextValue | null>(null);

export const useCompose = (): ComposeContextValue => {
  const value = useContext(ComposeContext);
  if (!value) {
    throw new Error("useCompose must be used within ComposeProvider");
  }
  return value;
};

export const ComposeProvider = ({ children }: Readonly<{ children: ReactNode }>) => {
  const [intent, setIntent] = useState<ComposeIntent>({ kind: "Closed" });
  const [lastPublished, setLastPublished] = useState<Status | null>(null);
  const [lastPublishToken, setLastPublishToken] = useState(0);

  const openNew = useCallback(() => {
    setIntent((current) => (current.kind === "Closed" ? { kind: "New" } : current));
  }, []);
  const openQuote = useCallback((quotedPreview: QuotedStatusPreview) => {
    setIntent({ kind: "Quote", quotedStatusId: quotedPreview.id, quotedPreview });
  }, []);
  const openEdit = useCallback((statusId: string) => {
    setIntent({ kind: "Edit", statusId });
  }, []);
  const close = useCallback(() => {
    setIntent({ kind: "Closed" });
  }, []);
  const markPublished = useCallback((status: Status) => {
    setLastPublished(status);
    setLastPublishToken((current) => current + 1);
  }, []);

  useKeyboardShortcuts([
    {
      key: "n",
      handler: () => {
        if (!isOverlayOpen()) {
          openNew();
        }
      },
    },
  ]);

  const value = useMemo(
    () => ({
      intent,
      lastPublished,
      lastPublishToken,
      openNew,
      openQuote,
      openEdit,
      close,
      markPublished,
    }),
    [intent, lastPublished, lastPublishToken, openNew, openQuote, openEdit, close, markPublished],
  );

  return <ComposeContext.Provider value={value}>{children}</ComposeContext.Provider>;
};
