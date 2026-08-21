import { forwardRef, useEffect, useImperativeHandle, useRef, useState } from "react";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import { PollDraft } from "@/domain/status/poll";
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

const POLL_EXPIRES = [
  { value: 3600, label: "1時間" },
  { value: 21_600, label: "6時間" },
  { value: 86_400, label: "1日" },
  { value: 259_200, label: "3日" },
  { value: 604_800, label: "7日" },
] as const;

export type ComposerSubmitInput = Readonly<{
  text: string;
  visibility: Visibility;
  spoilerText: string;
  sensitive: boolean;
  inReplyToId?: string;
  mediaIds: ReadonlyArray<string>;
  poll: PollDraft | null;
}>;

type ComposerProps = Readonly<{
  placeholder?: string;
  submitLabel?: string;
  initialVisibility?: Visibility;
  lockVisibility?: boolean;
  inReplyToId?: string;
  allowPoll?: boolean;
  disabled?: boolean;
  onSubmit: (input: ComposerSubmitInput) => Promise<void>;
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
    allowPoll = true,
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
  const [pollEnabled, setPollEnabled] = useState(false);
  const [poll, setPoll] = useState(PollDraft.empty);
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
  const pollReady = pollEnabled && PollDraft.isReady(poll);
  const canSubmit =
    (text.trim().length > 0 || readyMediaIds.length > 0 || pollReady) &&
    (!pollEnabled || PollDraft.isReady(poll)) &&
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
        mediaIds: pollEnabled ? [] : readyMediaIds,
        poll: pollEnabled && PollDraft.isReady(poll) ? { ...poll, options: PollDraft.filledOptions(poll) } : null,
      });
      setText("");
      setSpoilerText("");
      setShowCw(false);
      setPollEnabled(false);
      setPoll(PollDraft.empty());
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
            disabled={disabled || submitting || pollEnabled}
            onSelectFiles={handleSelectFiles}
            onRemove={handleRemoveMedia}
          />
          {pollEnabled ? (
            <div className="composer-poll">
              {poll.options.map((option, index) => (
                <div key={index} className="composer-poll-option">
                  <input
                    value={option}
                    onChange={(event) =>
                      setPoll((current) => PollDraft.setOption(current, index, event.target.value))
                    }
                    placeholder={`選択肢 ${index + 1}`}
                    disabled={disabled || submitting}
                  />
                  {poll.options.length > PollDraft.minOptions ? (
                    <button
                      type="button"
                      className="app-button app-button-secondary"
                      onClick={() => setPoll((current) => PollDraft.removeOption(current, index))}
                      disabled={disabled || submitting}
                    >
                      削除
                    </button>
                  ) : null}
                </div>
              ))}
              {poll.options.length < PollDraft.maxOptions ? (
                <button
                  type="button"
                  className="app-button app-button-secondary"
                  onClick={() => setPoll((current) => PollDraft.addOption(current))}
                  disabled={disabled || submitting}
                >
                  選択肢を追加
                </button>
              ) : null}
              <label className="composer-poll-meta">
                <span className="app-muted">期限</span>
                <select
                  value={poll.expiresIn}
                  onChange={(event) =>
                    setPoll((current) => ({ ...current, expiresIn: Number(event.target.value) }))
                  }
                  disabled={disabled || submitting}
                >
                  {POLL_EXPIRES.map((option) => (
                    <option key={option.value} value={option.value}>
                      {option.label}
                    </option>
                  ))}
                </select>
              </label>
              <label className="composer-cw">
                <input
                  type="checkbox"
                  checked={poll.multiple}
                  onChange={(event) =>
                    setPoll((current) => ({ ...current, multiple: event.target.checked }))
                  }
                  disabled={disabled || submitting}
                />
                複数選択可
              </label>
            </div>
          ) : null}
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
            {allowPoll ? (
              <label className="composer-cw">
                <input
                  type="checkbox"
                  checked={pollEnabled}
                  onChange={(event) => {
                    setPollEnabled(event.target.checked);
                    if (event.target.checked) {
                      clearMediaAttachments();
                    }
                  }}
                  disabled={disabled || submitting || readyMediaIds.length > 0}
                />
                アンケート
              </label>
            ) : null}
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
