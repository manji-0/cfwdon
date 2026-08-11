import type { DraftMediaAttachment } from "@/ui/composer/draft-media";
import { WebUiPhase } from "@/plan/phases";

type ComposerMediaPickerProps = Readonly<{
  attachments: ReadonlyArray<DraftMediaAttachment>;
  disabled?: boolean;
  onSelectFiles: (files: ReadonlyArray<File>) => void;
  onRemove: (localId: string) => void;
}>;

/** TODO(Phase 1): Enable file input, previews, and upload progress via `uploadMedia`. */
export const ComposerMediaPicker = ({
  attachments,
  disabled = false,
  onSelectFiles,
  onRemove,
}: ComposerMediaPickerProps) => (
  <div className="composer-media-picker" data-phase={WebUiPhase.timelineMedia}>
    <button
      type="button"
      className="app-button app-button-secondary"
      disabled={disabled}
      onClick={() => onSelectFiles([])}
    >
      メディアを添付（準備中）
    </button>
    {attachments.length > 0 ? (
      <ul className="composer-media-list">
        {attachments.map((attachment) => (
          <li key={attachment.localId}>
            <img src={attachment.previewUrl} alt="" />
            <button type="button" onClick={() => onRemove(attachment.localId)}>
              削除
            </button>
          </li>
        ))}
      </ul>
    ) : null}
  </div>
);
