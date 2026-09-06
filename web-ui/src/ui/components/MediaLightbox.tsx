import { useEffect } from "react";
import { MediaAttachment, type MediaAttachment as Media } from "@/domain/media/attachment";

type MediaLightboxProps = Readonly<{
  attachments: ReadonlyArray<Media>;
  index: number;
  onClose: () => void;
  onIndexChange: (index: number) => void;
}>;

const MediaBody = ({ media }: Readonly<{ media: Media }>) => {
  switch (media.kind) {
    case "Video":
    case "Gifv":
      return (
        <video
          className="media-lightbox-media"
          src={media.url}
          poster={media.previewUrl}
          controls
          autoPlay
          playsInline
        />
      );
    case "Audio":
      return (
        <div className="media-lightbox-audio">
          <audio src={media.url} controls autoPlay />
        </div>
      );
    case "Image":
    case "Unknown":
      return (
        <img
          className="media-lightbox-media"
          src={media.url}
          alt={media.description ?? MediaAttachment.label(media)}
        />
      );
  }
};

export const MediaLightbox = ({
  attachments,
  index,
  onClose,
  onIndexChange,
}: MediaLightboxProps) => {
  const media = attachments[index];

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
        return;
      }
      if (event.key === "ArrowLeft" && index > 0) {
        onIndexChange(index - 1);
        return;
      }
      if (event.key === "ArrowRight" && index < attachments.length - 1) {
        onIndexChange(index + 1);
      }
    };
    window.addEventListener("keydown", onKey);
    const previous = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      window.removeEventListener("keydown", onKey);
      document.body.style.overflow = previous;
    };
  }, [attachments.length, index, onClose, onIndexChange]);

  if (!media) {
    return null;
  }

  return (
    <div
      className="media-lightbox"
      role="dialog"
      aria-modal="true"
      aria-label={media.description ?? MediaAttachment.label(media)}
      onClick={onClose}
    >
      <button type="button" className="media-lightbox-close" onClick={onClose}>
        閉じる
      </button>
      {attachments.length > 1 && index > 0 ? (
        <button
          type="button"
          className="media-lightbox-nav is-prev"
          aria-label="前のメディア"
          onClick={(event) => {
            event.stopPropagation();
            onIndexChange(index - 1);
          }}
        >
          ‹
        </button>
      ) : null}
      <div className="media-lightbox-stage" onClick={(event) => event.stopPropagation()}>
        <MediaBody media={media} />
        {media.description ? <p className="media-lightbox-caption">{media.description}</p> : null}
        {attachments.length > 1 ? (
          <p className="media-lightbox-index app-muted">
            {index + 1} / {attachments.length}
          </p>
        ) : null}
      </div>
      {attachments.length > 1 && index < attachments.length - 1 ? (
        <button
          type="button"
          className="media-lightbox-nav is-next"
          aria-label="次のメディア"
          onClick={(event) => {
            event.stopPropagation();
            onIndexChange(index + 1);
          }}
        >
          ›
        </button>
      ) : null}
    </div>
  );
};
