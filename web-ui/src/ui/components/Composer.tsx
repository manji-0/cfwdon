import { forwardRef, useImperativeHandle, useRef, useState } from "react";
import type { Visibility } from "@/domain/status/visibility";
import { Visibility as VisibilityModel } from "@/domain/status/visibility";
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

  useImperativeHandle(
    ref,
    () => ({
      focus: () => {
        textareaRef.current?.focus();
      },
    }),
    [],
  );

  const handleSubmit = async () => {
    if (!text.trim() || submitting || disabled) {
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
      });
      setText("");
      setSpoilerText("");
      setShowCw(false);
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
          disabled={disabled || submitting || !text.trim()}
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
