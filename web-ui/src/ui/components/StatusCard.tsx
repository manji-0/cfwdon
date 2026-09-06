import { useState } from "react";
import { Link } from "react-router-dom";
import type { AccountRef } from "@/domain/account/account";
import type { AccountList } from "@/domain/lists/list";
import { MediaAttachment } from "@/domain/media/attachment";
import { StatusQuote } from "@/domain/status/quote";
import { Status } from "@/domain/status/status";
import type { StatusTranslation } from "@/domain/status/translation";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import { addListAccounts, fetchLists } from "@/infrastructure/api/lists";
import { translateStatus } from "@/infrastructure/api/status";
import { LinkPreviewCard } from "@/ui/components/LinkPreviewCard";
import { MediaLightbox } from "@/ui/components/MediaLightbox";
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
  editedAt,
  pinned,
}: Readonly<{
  account: AccountRef;
  createdAt: string;
  editedAt: string | null;
  pinned: boolean;
}>) => (
  <div className="status-card-header">
    <img className="status-avatar" src={account.avatar} alt="" loading="lazy" />
    <div className="status-card-meta">
      <Link className="status-display-name" to={`/profile/${account.id}`}>
        {account.displayName || account.username}
      </Link>
      <span className="status-acct">@{account.acct}</span>
      <span className="status-time">· {formatRelativeTime(createdAt)}</span>
      {editedAt ? <span className="status-time">· 編集済み</span> : null}
      {pinned ? <span className="status-time">· ピン留め</span> : null}
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
  onPin,
  onQuote,
  onEdit,
  onHistory,
  onMuteConversation,
  compact = false,
}: StatusCardProps) => {
  const body = Status.displayBody(status);
  const boostedBy = Status.boostedBy(status);
  const card = Status.visibleCard(status);
  const quote = body.quote && StatusQuote.isVisible(body.quote) ? body.quote.quotedStatus : null;
  const isOwn = Boolean(selfAccountId && body.account.id === selfAccountId);
  const [revealed, setRevealed] = useState(!body.sensitive && !body.spoilerText);
  const [menuOpen, setMenuOpen] = useState(false);
  const [translation, setTranslation] = useState<StatusTranslation | null>(null);
  const [translating, setTranslating] = useState(false);
  const [lightboxIndex, setLightboxIndex] = useState<number | null>(null);
  const [lists, setLists] = useState<ReadonlyArray<AccountList> | null>(null);
  const [listBusyId, setListBusyId] = useState<string | null>(null);
  const [listMessage, setListMessage] = useState("");
  const showContent = revealed || (!body.sensitive && !body.spoilerText);
  const lightboxItems = body.mediaAttachments.filter(MediaAttachment.isLightboxable);

  const handleReport = () => {
    const comment = window.prompt("通報の理由（任意）");
    if (comment === null) {
      return;
    }
    onReport?.(body, comment);
    setMenuOpen(false);
  };

  const handleTranslate = async () => {
    setMenuOpen(false);
    if (translation) {
      setTranslation(null);
      return;
    }
    setTranslating(true);
    const result = await translateStatus(body.id);
    setTranslating(false);
    if (result.isErr()) {
      window.alert(mastodonErrorMessage(result.error));
      return;
    }
    setTranslation(result.value);
  };

  const handleCopyLink = async () => {
    const permalink = `${window.location.origin}/app/status/${body.id}`;
    try {
      await navigator.clipboard.writeText(permalink);
      setMenuOpen(false);
    } catch {
      window.prompt("リンクをコピーしてください", permalink);
    }
  };

  const handleOpenLists = async () => {
    setListMessage("");
    if (lists) {
      setLists(null);
      return;
    }
    const result = await fetchLists();
    if (result.isErr()) {
      setListMessage(mastodonErrorMessage(result.error));
      return;
    }
    setLists(result.value);
  };

  const handleAddToList = async (listId: string) => {
    setListBusyId(listId);
    setListMessage("");
    const result = await addListAccounts(listId, [body.account.id]);
    setListBusyId(null);
    if (result.isErr()) {
      setListMessage(mastodonErrorMessage(result.error));
      return;
    }
    setListMessage("リストに追加しました");
  };

  const openLightbox = (mediaId: string) => {
    const index = lightboxItems.findIndex((item) => item.id === mediaId);
    if (index >= 0) {
      setLightboxIndex(index);
    }
  };

  return (
    <article className={`status-card${compact ? " status-card-compact" : ""}`}>
      {boostedBy ? (
        <p className="status-boost">
          <span aria-hidden="true">↻</span> {boostedBy.displayName || boostedBy.username} がブースト
        </p>
      ) : null}
      <AccountHeader
        account={body.account}
        createdAt={body.createdAt}
        editedAt={body.editedAt}
        pinned={body.pinned}
      />
      {body.spoilerText ? (
        <button type="button" className="status-spoiler-toggle" onClick={() => setRevealed((v) => !v)}>
          {showContent ? "警告を隠す" : `CW: ${body.spoilerText}`}
        </button>
      ) : null}
      {showContent ? (
        <>
          <StatusContent html={translation?.content ?? body.content} />
          {translation ? (
            <p className="status-translation-meta app-muted">
              {translation.provider
                ? `${translation.provider} が翻訳（${translation.detectedSourceLanguage || "auto"} → ${translation.language || "ja"}）`
                : "翻訳済み"}
            </p>
          ) : null}
          {body.mediaAttachments.length > 0 ? (
            <div className="status-media-grid">
              {body.mediaAttachments.map((media) =>
                MediaAttachment.isLightboxable(media) ? (
                  <button
                    key={media.id}
                    type="button"
                    className={
                      MediaAttachment.isVisual(media)
                        ? "status-media-thumb"
                        : "status-media-link"
                    }
                    onClick={() => openLightbox(media.id)}
                  >
                    {MediaAttachment.isVisual(media) ? (
                      <img
                        src={media.previewUrl}
                        alt={media.description ?? MediaAttachment.label(media)}
                        loading="lazy"
                      />
                    ) : (
                      `${MediaAttachment.label(media)}を再生`
                    )}
                  </button>
                ) : (
                  <a
                    key={media.id}
                    className="status-media-link"
                    href={media.url}
                    target="_blank"
                    rel="noreferrer"
                  >
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
          {quote ? (
            <Link className="status-quote" to={`/status/${quote.id}`}>
              <span className="status-quote-acct">@{quote.account.acct}</span>
              {quote.spoilerText ? (
                <span className="app-muted">CW: {quote.spoilerText}</span>
              ) : (
                <StatusContent html={quote.content} />
              )}
            </Link>
          ) : null}
        </>
      ) : null}
      <footer className="status-actions">
        <span className="status-action-group">
          <button type="button" className="status-action" onClick={() => onReply?.(body)} aria-label="返信">
            ↩
          </button>
          {body.repliesCount > 0 ? (
            <Link className="status-action-count" to={`/status/${body.id}`}>
              {body.repliesCount}
            </Link>
          ) : null}
        </span>
        <span className="status-action-group">
          <button
            type="button"
            className={`status-action${body.reblogged ? " is-active" : ""}`}
            onClick={() => onReblog?.(body)}
            aria-label="ブースト"
          >
            ↻
          </button>
          {body.reblogsCount > 0 ? (
            <Link className="status-action-count" to={`/status/${body.id}/reblogged-by`}>
              {body.reblogsCount}
            </Link>
          ) : null}
        </span>
        <span className="status-action-group">
          <button
            type="button"
            className={`status-action${body.favourited ? " is-active" : ""}`}
            onClick={() => onFavourite?.(body)}
            aria-label="いいね"
          >
            ♥
          </button>
          {body.favouritesCount > 0 ? (
            <Link className="status-action-count" to={`/status/${body.id}/favourited-by`}>
              {body.favouritesCount}
            </Link>
          ) : null}
        </span>
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
          {onQuote ? (
            <button type="button" onClick={() => onQuote(body)}>
              引用
            </button>
          ) : null}
          <button type="button" onClick={() => void handleCopyLink()}>
            リンクをコピー
          </button>
          <Link className="status-menu-link" to={`/status/${body.id}/quotes`} onClick={() => setMenuOpen(false)}>
            引用一覧
          </Link>
          {onMuteConversation ? (
            <button type="button" onClick={() => onMuteConversation(body)}>
              {body.muted ? "会話のミュートを解除" : "会話をミュート"}
            </button>
          ) : null}
          {!isOwn ? (
            <button type="button" onClick={() => void handleOpenLists()}>
              {lists ? "リストを閉じる" : "リストに追加"}
            </button>
          ) : null}
          {lists ? (
            <div className="status-menu-lists">
              {lists.length === 0 ? <p className="app-muted">リストはまだありません。</p> : null}
              {lists.map((list) => (
                <button
                  key={list.id}
                  type="button"
                  disabled={listBusyId === list.id}
                  onClick={() => void handleAddToList(list.id)}
                >
                  {listBusyId === list.id ? "追加中…" : list.title}
                </button>
              ))}
              {listMessage ? <p className="app-muted">{listMessage}</p> : null}
            </div>
          ) : null}
          <button type="button" onClick={() => void handleTranslate()} disabled={translating}>
            {translation ? "原文を表示" : translating ? "翻訳中…" : "翻訳"}
          </button>
          {body.editedAt && onHistory ? (
            <button type="button" onClick={() => onHistory(body)}>
              編集履歴
            </button>
          ) : null}
          {isOwn && onEdit ? (
            <button type="button" onClick={() => onEdit(body)}>
              編集
            </button>
          ) : null}
          {isOwn && onPin ? (
            <button type="button" onClick={() => onPin(body)}>
              {body.pinned ? "ピン留めを外す" : "ピン留め"}
            </button>
          ) : null}
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
      {lightboxIndex !== null ? (
        <MediaLightbox
          attachments={lightboxItems}
          index={lightboxIndex}
          onClose={() => setLightboxIndex(null)}
          onIndexChange={setLightboxIndex}
        />
      ) : null}
    </article>
  );
};
