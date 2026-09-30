import { useEffect, useRef, useState } from "react";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import { Visibility } from "@/domain/status/visibility";
import { createScheduledStatus } from "@/infrastructure/api/scheduled";
import {
  createStatus,
  fetchStatusSource,
  updateStatus,
} from "@/infrastructure/api/status";
import { Composer, type ComposerHandle, type ComposerSubmitInput } from "@/ui/components/Composer";
import { useCompose } from "@/ui/context/ComposeContext";
import { useConfirm } from "@/ui/context/ConfirmContext";
import { useViewCache } from "@/ui/context/ViewCacheContext";

/** Modal composer driven by `ComposeContext`; mount once inside `ComposeProvider`. */
export const ComposeSheet = () => {
  const { intent, close: onClose, markPublished: onPublished } = useCompose();
  const cache = useViewCache();
  const { alert, confirm } = useConfirm();
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

  const requestClose = async () => {
    if (composerRef.current?.isDirty()) {
      const discard = await confirm("入力中の内容は保存されません。破棄しますか？", {
        title: "下書きを破棄",
        confirmLabel: "破棄",
        danger: true,
      });
      if (!discard) {
        return;
      }
    }
    onClose();
  };

  useEffect(() => {
    if (intent.kind === "Closed") {
      return undefined;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") {
        return;
      }
      event.preventDefault();
      void requestClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

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
      onClick={() => void requestClose()}
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
            onCancel={() => void requestClose()}
            onSubmit={handleSubmit}
          />
        ) : null}
      </div>
    </div>
  );
};
