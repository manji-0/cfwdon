import { forwardRef, useEffect, useImperativeHandle, useRef, useState } from "react";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { Visibility } from "@/domain/status/visibility";
import { Visibility as VisibilityModel } from "@/domain/status/visibility";
import { uploadMedia } from "@/infrastructure/api/media";
import { ComposerMediaPicker } from "@/ui/components/ComposerMediaPicker";
import { ComposerMedia, type ComposerMediaItem } from "@/ui/composer/draft-media";
import { useSession } from "@/ui/context/SessionContext";
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
  lockVisibility?: boolean;
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

export const Composer = forwardRef<ComposerHandle, ComposerProps>(function Composer(
  {
    placeholder = "いまどうしてる？",
    submitLabel = "投稿",
    initialVisibility = VisibilityModel.public(),
    lockVisibility = false,
    inReplyToId,
    disabled = false,
    onSubmit,
  },
  ref,
) {
  const { session } = useSession();
  const account = session.kind === "Authenticated" ? session.account : null;
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

  const readyMediaIds = ComposerMedia.readyIds(mediaAttachments);
  const canSubmit =
    (text.trim().length > 0 || readyMediaIds.length > 0) &&
    !ComposerMedia.hasUploading(mediaAttachments) &&
    !submitting &&
    !disabled;

  const queueUpload = (file: File, localId: string) => {
    void uploadMedia(file).then((result) => {
      setMediaAttachments((current) =>
        ComposerMedia.complete(
          current,
          localId,
          result.map((media) => media.id).mapErr(mastodonErrorMessage),
        ),
      );
    });
  };

  const handleSelectFiles = (files: ReadonlyArray<File>) => {
    const created = files.map((file) => {
      const item = ComposerMedia.uploading(
        ComposerMedia.createLocalId(),
        file,
        URL.createObjectURL(file),
      );
      queueUpload(file, item.localId);
      return item;
    });
    setMediaAttachments((current) => ComposerMedia.append(current, created));
  };

  const handleRemoveMedia = (localId: string) => {
    setMediaAttachments((current) => {
      const target = ComposerMedia.member(current, localId);
      if (target) {
        URL.revokeObjectURL(target.previewUrl);
      }
      return ComposerMedia.remove(current, localId);
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
      <div className="composer-main">
        {account ? (
          <img
            className="status-avatar composer-avatar"
            src={account.avatar}
            alt=""
          />
        ) : null}
        <div className="composer-fields">
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
            {lockVisibility ? null : (
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
            )}
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
        </div>
      </div>
    </section>
  );
});
