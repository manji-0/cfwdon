import { forwardRef, useEffect, useImperativeHandle, useRef, useState } from "react";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { Visibility } from "@/domain/status/visibility";
import { Visibility as VisibilityModel } from "@/domain/status/visibility";
import { uploadMedia } from "@/infrastructure/api/media";
import { ComposerMediaPicker } from "@/ui/components/ComposerMediaPicker";
import { ComposerMedia, type ComposerMediaItem } from "@/ui/composer/draft-media";
import { isSubmitShortcut, modKeyLabel } from "@/ui/lib/keyboard";

const VISIBILITY_OPTIONS: ReadonlyArray<Visibility> = [
  VisibilityModel.public(),
  VisibilityModel.unlisted(),
  VisibilityModel.private(),
  VisibilityModel.direct(),
];

type ComposerProps = Readonly<{
  placeholder?: string;
  submitLabel?: string;
  initialVisibility?: Visibility;
  inReplyToId?: string;
  disabled?: boolean;
  onSubmit: (input: {
    text: string;
    visibility: Visibility;
    spoilerText: string;
    sensitive: boolean;
    inReplyToId?: string;
    mediaIds: ReadonlyArray<string>;
  }) => Promise<void>;
}>;

export type ComposerHandle = Readonly<{
  focus: () => void;
}>;

const createLocalId = (): string =>
  typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : `${Date.now()}-${Math.random().toString(16).slice(2)}`;

export const Composer = forwardRef<ComposerHandle, ComposerProps>(function Composer(
  {
    placeholder = "いまどうしてる？",
    submitLabel = "投稿",
    initialVisibility = VisibilityModel.public(),
    inReplyToId,
    disabled = false,
    onSubmit,
  },
  ref,
) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const [text, setText] = useState("");
  const [visibility, setVisibility] = useState<Visibility>(initialVisibility);
  const [spoilerText, setSpoilerText] = useState("");
  const [showCw, setShowCw] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState("");
  const [mediaAttachments, setMediaAttachments] = useState<ReadonlyArray<ComposerMediaItem>>([]);
  const mediaAttachmentsRef = useRef(mediaAttachments);
  mediaAttachmentsRef.current = mediaAttachments;

  useImperativeHandle(
    ref,
    () => ({
      focus: () => {
        textareaRef.current?.focus();
      },
    }),
    [],
  );

  useEffect(
    () => () => {
      for (const attachment of mediaAttachmentsRef.current) {
        URL.revokeObjectURL(attachment.previewUrl);
      }
    },
    [],
  );

  const readyMediaIds = mediaAttachments
    .filter((attachment) => attachment.status === "ready" && attachment.mediaId)
    .map((attachment) => attachment.mediaId as string);
  const hasUploadingMedia = mediaAttachments.some((attachment) => attachment.status === "uploading");
  const canSubmit =
    (text.trim().length > 0 || readyMediaIds.length > 0) && !hasUploadingMedia && !submitting && !disabled;

  const queueUpload = (file: File, localId: string) => {
    void uploadMedia(file).then((result) => {
      setMediaAttachments((current) =>
        current.map((attachment) => {
          if (attachment.localId !== localId) {
            return attachment;
          }
          if (result.isErr()) {
            return {
              ...attachment,
              status: "failed",
              errorMessage: mastodonErrorMessage(result.error),
            };
          }
          return {
            ...attachment,
            status: "ready",
            mediaId: result.value.id,
          };
        }),
      );
    });
  };

  const handleSelectFiles = (files: ReadonlyArray<File>) => {
    const remaining = ComposerMedia.maxAttachments - mediaAttachments.length;
    if (remaining <= 0) {
      return;
    }
    const selected = files.slice(0, remaining);
    const nextItems = selected.map((file) => {
      const localId = createLocalId();
      const previewUrl = URL.createObjectURL(file);
      queueUpload(file, localId);
      return {
        localId,
        file,
        previewUrl,
        status: "uploading" as const,
      };
    });
    setMediaAttachments((current) => [...current, ...nextItems]);
  };

  const handleRemoveMedia = (localId: string) => {
    setMediaAttachments((current) => {
      const target = current.find((attachment) => attachment.localId === localId);
      if (target) {
        URL.revokeObjectURL(target.previewUrl);
      }
      return current.filter((attachment) => attachment.localId !== localId);
    });
  };

  const clearMediaAttachments = () => {
    setMediaAttachments((current) => {
      for (const attachment of current) {
        URL.revokeObjectURL(attachment.previewUrl);
      }
      return [];
    });
  };

  const handleSubmit = async () => {
    if (!canSubmit) {
      return;
    }
    setSubmitting(true);
    setError("");
    try {
      await onSubmit({
        text: text.trim(),
        visibility,
        spoilerText: showCw ? spoilerText.trim() : "",
        sensitive: showCw && spoilerText.trim().length > 0,
        inReplyToId,
        mediaIds: readyMediaIds,
      });
      setText("");
      setSpoilerText("");
      setShowCw(false);
      clearMediaAttachments();
    } catch (submitError) {
      setError(submitError instanceof Error ? submitError.message : "投稿に失敗しました");
    } finally {
      setSubmitting(false);
    }
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (isSubmitShortcut(event)) {
      event.preventDefault();
      void handleSubmit();
      return;
    }
    if (event.key === "Escape") {
      event.currentTarget.blur();
    }
  };

  return (
    <section className="app-composer" aria-label="新規投稿">
      <textarea
        ref={textareaRef}
        value={text}
        onChange={(event) => setText(event.target.value)}
        onKeyDown={handleKeyDown}
        placeholder={placeholder}
        disabled={disabled || submitting}
      />
      <ComposerMediaPicker
        attachments={mediaAttachments}
        disabled={disabled || submitting}
        onSelectFiles={handleSelectFiles}
        onRemove={handleRemoveMedia}
      />
      <div className="composer-toolbar">
        <label className="composer-visibility">
          <span className="app-muted">公開範囲</span>
          <select
            value={VisibilityModel.toApi(visibility)}
            onChange={(event) => setVisibility(VisibilityModel.fromApi(event.target.value))}
            disabled={disabled || submitting}
          >
            {VISIBILITY_OPTIONS.map((option) => (
              <option key={option.kind} value={VisibilityModel.toApi(option)}>
                {VisibilityModel.label(option)}
              </option>
            ))}
          </select>
        </label>
        <label className="composer-cw">
          <input
            type="checkbox"
            checked={showCw}
            onChange={(event) => setShowCw(event.target.checked)}
            disabled={disabled || submitting}
          />
          CW
        </label>
        <span className="composer-shortcut-hint app-muted" aria-hidden="true">
          {modKeyLabel()}↵ で{submitLabel}
        </span>
        <button
          type="button"
          className="app-button"
          onClick={() => void handleSubmit()}
          disabled={!canSubmit}
        >
          {submitting ? "送信中…" : submitLabel}
        </button>
      </div>
      {showCw ? (
        <input
          className="composer-spoiler"
          value={spoilerText}
          onChange={(event) => setSpoilerText(event.target.value)}
          placeholder="コンテンツ警告"
          disabled={disabled || submitting}
        />
      ) : null}
      {error ? <p className="app-error">{error}</p> : null}
    </section>
  );
});
