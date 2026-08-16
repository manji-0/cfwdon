import { useState } from "react";
import { Link } from "react-router-dom";
import type { AccountRef } from "@/domain/account/account";
import type { Status } from "@/domain/status/status";
import { Status as StatusModel } from "@/domain/status/status";
import { PreviewCard } from "@/domain/status/preview-card";
import { LinkPreviewCard } from "@/ui/components/LinkPreviewCard";
import { StatusContent } from "@/ui/components/StatusContent";
import { formatRelativeTime } from "@/ui/lib/time";

type StatusCardProps = Readonly<{
  status: Status;
  onFavourite?: (status: Status) => void;
  onReblog?: (status: Status) => void;
  onBookmark?: (status: Status) => void;
  onReply?: (status: Status) => void;
  compact?: boolean;
}>;

const AccountHeader = ({
  account,
  createdAt,
}: Readonly<{ account: AccountRef; createdAt: string }>) => (
  <div className="status-card-header">
    <img className="status-avatar" src={account.avatar} alt="" loading="lazy" />
    <div className="status-card-meta">
      <Link className="status-display-name" to={`/profile/${account.id}`}>
        {account.displayName || account.username}
      </Link>
      <span className="status-acct">@{account.acct}</span>
      <span className="status-time">· {formatRelativeTime(createdAt)}</span>
    </div>
  </div>
);

export const StatusCard = ({
  status,
  onFavourite,
  onReblog,
  onBookmark,
  onReply,
  compact = false,
}: StatusCardProps) => {
  const [revealed, setRevealed] = useState(!status.sensitive && !status.spoilerText);
  const body = StatusModel.displayBody(status);
  const boostedBy = StatusModel.boostedBy(status);
  const showContent = revealed || (!body.sensitive && !body.spoilerText);

  return (
    <article className={`status-card${compact ? " status-card-compact" : ""}`}>
      {boostedBy ? (
        <p className="status-boost">
          <span aria-hidden="true">↻</span> {boostedBy.displayName || boostedBy.username} がブースト
        </p>
      ) : null}
      <AccountHeader account={body.account} createdAt={body.createdAt} />
      {body.spoilerText ? (
        <button type="button" className="status-spoiler-toggle" onClick={() => setRevealed((v) => !v)}>
          {showContent ? "警告を隠す" : `CW: ${body.spoilerText}`}
        </button>
      ) : null}
      {showContent ? (
        <>
          <StatusContent html={body.content} />
          {body.mediaAttachments.length > 0 ? (
            <div className="status-media-grid">
              {body.mediaAttachments.map((media) =>
                media.type === "image" || media.type === "gifv" ? (
                  <a key={media.id} href={media.url} target="_blank" rel="noreferrer">
                    <img src={media.previewUrl} alt={media.description ?? ""} loading="lazy" />
                  </a>
                ) : (
                  <a key={media.id} className="status-media-link" href={media.url} target="_blank" rel="noreferrer">
                    {media.type} を開く
                  </a>
                ),
              )}
            </div>
          ) : null}
          {PreviewCard.isVisible(body) ? <LinkPreviewCard card={body.card!} /> : null}
        </>
      ) : null}
      <footer className="status-actions">
        <button type="button" className="status-action" onClick={() => onReply?.(body)} aria-label="返信">
          ↩ {body.repliesCount > 0 ? body.repliesCount : ""}
        </button>
        <button
          type="button"
          className={`status-action${body.reblogged ? " is-active" : ""}`}
          onClick={() => onReblog?.(body)}
          aria-label="ブースト"
        >
          ↻ {body.reblogsCount > 0 ? body.reblogsCount : ""}
        </button>
        <button
          type="button"
          className={`status-action${body.favourited ? " is-active" : ""}`}
          onClick={() => onFavourite?.(body)}
          aria-label="いいね"
        >
          ♥ {body.favouritesCount > 0 ? body.favouritesCount : ""}
        </button>
        {onBookmark ? (
          <button
            type="button"
            className={`status-action${body.bookmarked ? " is-active" : ""}`}
            onClick={() => onBookmark(body)}
            aria-label="ブックマーク"
          >
            {body.bookmarked ? "★" : "☆"}
          </button>
        ) : null}
        <Link className="status-action" to={`/status/${body.id}`} aria-label="スレッドを開く">
          ⧉
        </Link>
      </footer>
    </article>
  );
};
