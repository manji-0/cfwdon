import { useCallback, useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { Conversation } from "@/domain/conversations/conversation";
import { conversationTitle } from "@/domain/conversations/participants";
import { countUnreadConversations } from "@/domain/conversations/unread";
import { fetchConversations } from "@/infrastructure/api/conversations";
import { StreamingUser } from "@/infrastructure/streaming/mastodon-stream";
import { AppShell } from "@/ui/components/AppShell";
import { useUnreadMessages } from "@/ui/context/UnreadMessagesContext";
import { formatRelativeTime } from "@/ui/lib/time";

const upsertConversation = (
  current: ReadonlyArray<Conversation>,
  next: Conversation,
): ReadonlyArray<Conversation> => [next, ...current.filter((item) => item.id !== next.id)];

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

  useEffect(() => {
    const subscription = StreamingUser.subscribe((event) => {
      if (event.kind === "conversation") {
        setConversations((current) => {
          const next = upsertConversation(current, event.conversation);
          setUnreadCount(countUnreadConversations(next));
          return next;
        });
      }
    });
    return () => subscription.close();
  }, [setUnreadCount]);

  const openConversation = (conversation: Conversation) => {
    navigate(`/messages/${conversation.id}`);
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
      <div className="messages-toolbar">
        <Link className="app-button" to="/messages/new">
          新しいメッセージ
        </Link>
      </div>
      {error ? <p className="app-error">{error}</p> : null}
      {loading ? <div className="app-status">読み込み中…</div> : null}
      <div className="conversation-list">
        {conversations.map((conversation) => {
          const preview = conversation.lastStatus?.content.replace(/<[^>]+>/g, "") ?? "";
          return (
            <button
              key={conversation.id}
              type="button"
              className={`conversation-row${conversation.unread ? " is-unread" : ""}`}
              onClick={() => openConversation(conversation)}
            >
              {conversation.accounts[0] ? (
                <img
                  className="status-avatar"
                  src={conversation.accounts[0].avatar}
                  alt=""
                  loading="lazy"
                />
              ) : (
                <div className="status-avatar conversation-avatar-fallback" />
              )}
              <div className="conversation-meta">
                <div className="conversation-header">
                  <span className="status-display-name">
                    {conversationTitle(conversation.accounts)}
                  </span>
                  {conversation.lastStatus ? (
                    <span className="app-muted">
                      {formatRelativeTime(conversation.lastStatus.createdAt)}
                    </span>
                  ) : null}
                </div>
                {conversation.accounts.length > 0 ? (
                  <span className="status-acct">
                    {conversation.accounts.map((account) => `@${account.acct}`).join(" ")}
                  </span>
                ) : null}
                {preview ? <p className="conversation-preview app-muted">{preview}</p> : null}
                {conversation.lastStatus ? null : (
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
    </AppShell>
  );
};
