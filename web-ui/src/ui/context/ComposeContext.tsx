import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { QuotedStatusPreview } from "@/domain/status/quote";
import { Status } from "@/domain/status/status";
import { Visibility } from "@/domain/status/visibility";
import { createScheduledStatus } from "@/infrastructure/api/scheduled";
import {
  createStatus,
  fetchStatusSource,
  updateStatus,
} from "@/infrastructure/api/status";
import { Composer, type ComposerHandle, type ComposerSubmitInput } from "@/ui/components/Composer";
import { useConfirm } from "@/ui/context/ConfirmContext";
import { useViewCache } from "@/ui/context/ViewCacheContext";
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
    () => ({ intent, lastPublished, lastPublishToken, openNew, openQuote, openEdit, close }),
    [intent, lastPublished, lastPublishToken, openNew, openQuote, openEdit, close],
  );

  return (
    <ComposeContext.Provider value={value}>
      {children}
      <ComposeSheet
        intent={intent}
        onClose={close}
        onPublished={(status) => {
          setLastPublished(status);
          setLastPublishToken((current) => current + 1);
        }}
      />
    </ComposeContext.Provider>
  );
};

const ComposeSheet = ({
  intent,
  onClose,
  onPublished,
}: Readonly<{
  intent: ComposeIntent;
  onClose: () => void;
  onPublished: (status: Status) => void;
}>) => {
  const cache = useViewCache();
  const { alert } = useConfirm();
  const composerRef = useRef<ComposerHandle>(null);
  const [editText, setEditText] = useState("");
  const [editSpoiler, setEditSpoiler] = useState("");
  const [editReady, setEditReady] = useState(false);
  const [loadError, setLoadError] = useState("");

  useEffect(() => {
    if (intent.kind !== "Edit") {
      setEditText("");
      setEditSpoiler("");
      setEditReady(false);
      setLoadError("");
      return;
    }
    let active = true;
    setEditReady(false);
    setLoadError("");
    void fetchStatusSource(intent.statusId).then((result) => {
      if (!active) {
        return;
      }
      if (result.isErr()) {
        setLoadError(mastodonErrorMessage(result.error));
        return;
      }
      setEditText(result.value.text);
      setEditSpoiler(result.value.spoilerText);
      setEditReady(true);
    });
    return () => {
      active = false;
    };
  }, [intent]);

  useEffect(() => {
    if (intent.kind === "Closed") {
      return;
    }
    if (intent.kind === "Edit" && !editReady) {
      return;
    }
    composerRef.current?.focus();
  }, [intent, editReady]);

  useEffect(() => {
    if (intent.kind === "Closed") {
      return undefined;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") {
        return;
      }
      event.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [intent, onClose]);

  if (intent.kind === "Closed") {
    return null;
  }

  const handleSubmit = async (input: ComposerSubmitInput) => {
    if (intent.kind === "Edit") {
      const result = await updateStatus(intent.statusId, {
        text: input.text,
        spoilerText: input.spoilerText,
        sensitive: input.sensitive,
        mediaIds: input.mediaIds,
      });
      if (result.isErr()) {
        throw new Error(mastodonErrorMessage(result.error));
      }
      cache.patchStatus(result.value);
      onPublished(result.value);
      onClose();
      return;
    }
    if (input.scheduledAt) {
      const result = await createScheduledStatus({
        text: input.text,
        visibility: Visibility.toApi(input.visibility),
        scheduledAt: input.scheduledAt,
        spoilerText: input.spoilerText,
        sensitive: input.sensitive,
        language: input.language,
        mediaIds: input.mediaIds,
        poll: input.poll,
        quotedStatusId: intent.kind === "Quote" ? intent.quotedStatusId : undefined,
      });
      if (result.isErr()) {
        throw new Error(mastodonErrorMessage(result.error));
      }
      onClose();
      await alert("予約しました", { title: "予約投稿" });
      return;
    }
    const result = await createStatus({
      text: input.text,
      visibility: Visibility.toApi(input.visibility),
      spoilerText: input.spoilerText,
      sensitive: input.sensitive,
      language: input.language,
      mediaIds: input.mediaIds,
      poll: input.poll,
      quotedStatusId: intent.kind === "Quote" ? intent.quotedStatusId : undefined,
    });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    onPublished(result.value);
    onClose();
  };

  const composerKey =
    intent.kind === "Edit"
      ? `edit-${intent.statusId}`
      : intent.kind === "Quote"
        ? `quote-${intent.quotedStatusId}`
        : "new";

  return (
    <div
      className="compose-overlay"
      data-app-overlay="true"
      role="presentation"
      onClick={onClose}
    >
      <div
        className="compose-sheet app-card"
        role="dialog"
        aria-modal="true"
        aria-label={intent.kind === "Edit" ? "投稿を編集" : "新規投稿"}
        onClick={(event) => event.stopPropagation()}
      >
        {intent.kind === "Edit" && !editReady && !loadError ? (
          <p className="app-muted">読み込み中…</p>
        ) : null}
        {loadError ? <p className="app-error">{loadError}</p> : null}
        {intent.kind !== "Edit" || editReady ? (
          <Composer
            key={composerKey}
            ref={composerRef}
            submitLabel={intent.kind === "Edit" ? "保存" : "投稿"}
            applyPostingDefaults={intent.kind !== "Edit"}
            initialText={intent.kind === "Edit" ? editText : ""}
            initialSpoilerText={intent.kind === "Edit" ? editSpoiler : ""}
            quotedStatusId={intent.kind === "Quote" ? intent.quotedStatusId : undefined}
            quotedPreview={intent.kind === "Quote" ? intent.quotedPreview : null}
            allowSchedule={intent.kind !== "Edit"}
            onCancel={onClose}
            onSubmit={handleSubmit}
          />
        ) : null}
      </div>
    </div>
  );
};
