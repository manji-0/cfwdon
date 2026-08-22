import { useRef } from "react";
import { WebUiPhase } from "@/plan/phases";
import { ComposerMedia, type ComposerMediaItem } from "@/ui/composer/draft-media";

type ComposerMediaPickerProps = Readonly<{
  attachments: ReadonlyArray<ComposerMediaItem>;
  disabled?: boolean;
  onSelectFiles: (files: ReadonlyArray<File>) => void;
  onRemove: (localId: string) => void;
  onDescriptionChange?: (localId: string, description: string) => void;
  onDescriptionBlur?: (localId: string) => void;
}>;

export const ComposerMediaPicker = ({
  attachments,
  disabled = false,
  onSelectFiles,
  onRemove,
  onDescriptionChange,
  onDescriptionBlur,
}: ComposerMediaPickerProps) => {
  const fileInputRef = useRef<HTMLInputElement>(null);
  const atLimit = attachments.length >= ComposerMedia.maxAttachments;

  return (
    <div className="composer-media-picker" data-phase={WebUiPhase.timelineMedia}>
      <input
        ref={fileInputRef}
        type="file"
        accept={ComposerMedia.accept}
        multiple
        hidden
        onChange={(event) => {
          const files = Array.from(event.target.files ?? []);
          event.target.value = "";
          if (files.length > 0) {
            onSelectFiles(files);
          }
        }}
      />
      <button
        type="button"
        className="app-button app-button-secondary"
        disabled={disabled || atLimit}
        onClick={() => fileInputRef.current?.click()}
      >
        メディアを添付
      </button>
      {atLimit ? (
        <p className="app-muted composer-media-limit">
          添付は最大 {ComposerMedia.maxAttachments} 件です
        </p>
      ) : null}
      {attachments.length > 0 ? (
        <ul className="composer-media-list">
          {attachments.map((attachment) => (
            <li key={attachment.localId} className="composer-media-item">
              {attachment.file.type.startsWith("image/") ? (
                <img src={attachment.previewUrl} alt={attachment.kind === "Ready" ? attachment.description : ""} />
              ) : (
                <div className="composer-media-file">
                  <span>{attachment.file.name}</span>
                  <span className="app-muted">{attachment.file.type || "file"}</span>
                </div>
              )}
              <div className="composer-media-meta">
                {attachment.kind === "Uploading" ? (
                  <span className="app-muted">アップロード中…</span>
                ) : null}
                {attachment.kind === "Failed" ? (
                  <span className="app-error">{attachment.message}</span>
                ) : null}
                {attachment.kind === "Ready" ? (
                  <label className="composer-media-alt">
                    <span className="app-muted">代替テキスト</span>
                    <input
                      value={attachment.description}
                      onChange={(event) => onDescriptionChange?.(attachment.localId, event.target.value)}
                      onBlur={() => onDescriptionBlur?.(attachment.localId)}
                      placeholder="画像の説明"
                      disabled={disabled}
                    />
                  </label>
                ) : null}
                <button
                  type="button"
                  className="app-button app-button-secondary"
                  onClick={() => onRemove(attachment.localId)}
                  disabled={disabled || attachment.kind === "Uploading"}
                >
                  削除
                </button>
              </div>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
};
