import { useCallback, useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { Conversation } from "@/domain/conversations/conversation";
import { countUnreadConversations } from "@/domain/conversations/unread";
import {
  fetchConversations,
  markConversationRead,
} from "@/infrastructure/api/conversations";
import { WebUiPhase } from "@/plan/phases";
import { AppShell } from "@/ui/components/AppShell";
import { useUnreadMessages } from "@/ui/context/UnreadMessagesContext";
import { formatRelativeTime } from "@/ui/lib/time";

export const MessagesPage = () => {
  const navigate = useNavigate();
  const { setUnreadCount, refreshUnreadCount } = useUnreadMessages();
  const [conversations, setConversations] = useState<ReadonlyArray<Conversation>>([]);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState("");

  const loadConversations = useCallback(async (options?: { maxId?: string; replace?: boolean }) => {
    const result = await fetchConversations({ maxId: options?.maxId, limit: 20 });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    setConversations((current) => {
      const next =
        options?.replace || !options?.maxId ? result.value : [...current, ...result.value];
      if (options?.replace || !options?.maxId) {
        setUnreadCount(countUnreadConversations(next));
      }
      return next;
    });
  }, [setUnreadCount]);

  useEffect(() => {
    let active = true;
    setLoading(true);
    void loadConversations({ replace: true })
      .catch((loadError) => {
        if (active) {
          setError(loadError instanceof Error ? loadError.message : "メッセージの読み込みに失敗しました");
        }
      })
      .finally(() => {
        if (active) {
          setLoading(false);
        }
      });
    return () => {
      active = false;
    };
  }, [loadConversations]);

  useEffect(() => {
    refreshUnreadCount();
  }, [refreshUnreadCount]);

  const openConversation = async (conversation: Conversation) => {
    if (conversation.unread) {
      const result = await markConversationRead(conversation.id);
      if (result.isOk()) {
        setConversations((current) => {
          const next = current.map((item) =>
            item.id === conversation.id ? result.value : item,
          );
          setUnreadCount(countUnreadConversations(next));
          return next;
        });
      }
    }
    if (conversation.lastStatus) {
      navigate(`/status/${conversation.lastStatus.id}`);
    }
  };

  const handleLoadMore = async () => {
    const last = conversations.at(-1);
    if (!last || loadingMore) {
      return;
    }
    setLoadingMore(true);
    setError("");
    try {
      await loadConversations({ maxId: last.id });
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : "続きの読み込みに失敗しました");
    } finally {
      setLoadingMore(false);
    }
  };

  return (
    <AppShell title="メッセージ">
      <div data-phase={WebUiPhase.collections}>
        {error ? <p className="app-error">{error}</p> : null}
        {loading ? <div className="app-status">読み込み中…</div> : null}
        <div className="conversation-list">
          {conversations.map((conversation) => {
            const peer = conversation.accounts[0];
            const preview = conversation.lastStatus?.content.replace(/<[^>]+>/g, "") ?? "";
            return (
              <button
                key={conversation.id}
                type="button"
                className={`conversation-row${conversation.unread ? " is-unread" : ""}`}
                onClick={() => void openConversation(conversation)}
              >
                {peer ? (
                  <img className="status-avatar" src={peer.avatar} alt="" loading="lazy" />
                ) : (
                  <div className="status-avatar conversation-avatar-fallback" />
                )}
                <div className="conversation-meta">
                  <div className="conversation-header">
                    <span className="status-display-name">
                      {peer ? peer.displayName || peer.username : "会話"}
                    </span>
                    {conversation.lastStatus ? (
                      <span className="app-muted">
                        {formatRelativeTime(conversation.lastStatus.createdAt)}
                      </span>
                    ) : null}
                  </div>
                  {peer ? <span className="status-acct">@{peer.acct}</span> : null}
                  {preview ? <p className="conversation-preview app-muted">{preview}</p> : null}
                  {conversation.lastStatus ? (
                    <Link
                      className="conversation-thread-link"
                      to={`/status/${conversation.lastStatus.id}`}
                      onClick={(event) => event.stopPropagation()}
                    >
                      スレッドを開く
                    </Link>
                  ) : (
                    <span className="app-muted">最新の投稿がありません</span>
                  )}
                </div>
              </button>
            );
          })}
        </div>
        {!loading && conversations.length === 0 ? (
          <div className="app-card">
            <p className="app-muted">ダイレクトメッセージはまだありません。</p>
          </div>
        ) : null}
        {conversations.length > 0 ? (
          <div className="timeline-footer">
            <button
              type="button"
              className="app-button app-button-secondary"
              onClick={() => void handleLoadMore()}
              disabled={loadingMore}
            >
              {loadingMore ? "読み込み中…" : "もっと見る"}
            </button>
          </div>
        ) : null}
      </div>
    </AppShell>
  );
};
