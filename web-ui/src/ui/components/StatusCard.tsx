import { useState } from "react";
import { Link } from "react-router-dom";
import type { AccountRef } from "@/domain/account/account";
import { MediaAttachment } from "@/domain/media/attachment";
import { Status } from "@/domain/status/status";
import { LinkPreviewCard } from "@/ui/components/LinkPreviewCard";
import { PollCard } from "@/ui/components/PollCard";
import { StatusContent } from "@/ui/components/StatusContent";
import type { StatusActionHandlers } from "@/ui/hooks/useStatusActions";
import { formatRelativeTime } from "@/ui/lib/time";

type StatusCardProps = Readonly<{
  status: Status;
  compact?: boolean;
}> &
  Partial<StatusActionHandlers>;

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
  selfAccountId = null,
  onFavourite,
  onReblog,
  onBookmark,
  onReply,
  onDelete,
  onMute,
  onBlock,
  onReport,
  onVotePoll,
  compact = false,
}: StatusCardProps) => {
  const body = Status.displayBody(status);
  const boostedBy = Status.boostedBy(status);
  const card = Status.visibleCard(status);
  const isOwn = Boolean(selfAccountId && body.account.id === selfAccountId);
  const [revealed, setRevealed] = useState(!body.sensitive && !body.spoilerText);
  const [menuOpen, setMenuOpen] = useState(false);
  const showContent = revealed || (!body.sensitive && !body.spoilerText);

  const handleReport = () => {
    const comment = window.prompt("通報の理由（任意）");
    if (comment === null) {
      return;
    }
    onReport?.(body, comment);
    setMenuOpen(false);
  };

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
                MediaAttachment.isVisual(media) ? (
                  <a key={media.id} href={media.url} target="_blank" rel="noreferrer">
                    <img src={media.previewUrl} alt={media.description ?? ""} loading="lazy" />
                  </a>
                ) : (
                  <a key={media.id} className="status-media-link" href={media.url} target="_blank" rel="noreferrer">
                    {MediaAttachment.label(media)} を開く
                  </a>
                ),
              )}
            </div>
          ) : null}
          {body.poll ? (
            <PollCard
              poll={body.poll}
              onVote={onVotePoll ? (choices) => onVotePoll(body, choices) : undefined}
            />
          ) : null}
          {card ? <LinkPreviewCard card={card} /> : null}
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
        <button
          type="button"
          className={`status-action${menuOpen ? " is-active" : ""}`}
          aria-label="その他"
          aria-expanded={menuOpen}
          onClick={() => setMenuOpen((current) => !current)}
        >
          …
        </button>
      </footer>
      {menuOpen ? (
        <div className="status-menu">
          {isOwn && onDelete ? (
            <button type="button" onClick={() => onDelete(body)}>
              削除
            </button>
          ) : null}
          {!isOwn && onMute ? (
            <button type="button" onClick={() => onMute(body)}>
              ミュート
            </button>
          ) : null}
          {!isOwn && onBlock ? (
            <button type="button" onClick={() => onBlock(body)}>
              ブロック
            </button>
          ) : null}
          {!isOwn && onReport ? (
            <button type="button" onClick={handleReport}>
              通報
            </button>
          ) : null}
        </div>
      ) : null}
    </article>
  );
};
