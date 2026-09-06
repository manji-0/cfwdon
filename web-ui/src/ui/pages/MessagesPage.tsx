import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useNavigate } from "react-router";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import { Conversation } from "@/domain/conversations/conversation";
import { ConversationSet } from "@/domain/conversations/conversation-set";
import { conversationTitle } from "@/domain/conversations/participants";
import { Status } from "@/domain/status/status";
import { fetchConversations } from "@/infrastructure/api/conversations";
import { StreamingUser } from "@/infrastructure/streaming/mastodon-stream";
import { AppShell } from "@/ui/components/AppShell";
import { LoadMoreFooter } from "@/ui/components/LoadMoreFooter";
import { useUnreadMessages } from "@/ui/context/UnreadMessagesContext";
import { usePagePrefetch } from "@/ui/hooks/usePagePrefetch";
import { TIMELINE_PAGE_LIMIT, pageHasMore } from "@/ui/lib/pagination";
import { formatRelativeTime } from "@/ui/lib/time";

export const MessagesPage = () => {
  const navigate = useNavigate();
  const { setUnreadCount, refreshUnreadCount } = useUnreadMessages();
  const [conversations, setConversations] = useState(ConversationSet.empty);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const [error, setError] = useState("");
  const loadingMoreRef = useRef(false);
  const conversationsRef = useRef(conversations);
  conversationsRef.current = conversations;
  const prefetch = usePagePrefetch(async (maxId: string) => {
    const result = await fetchConversations({ maxId, limit: TIMELINE_PAGE_LIMIT });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    return result.value;
  });

  const loadConversations = useCallback(async () => {
    const result = await fetchConversations({ limit: TIMELINE_PAGE_LIMIT });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    setHasMore(pageHasMore(result.value.length));
    const next = ConversationSet.replace(result.value);
    setUnreadCount(ConversationSet.unreadCount(next));
    setConversations(next);
    prefetch.prepareNext(next, result.value.length);
  }, [prefetch, setUnreadCount]);

  useEffect(() => {
    let active = true;
    setLoading(true);
    void loadConversations()
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
      if (event.kind === "Conversation") {
        setConversations((current) => {
          const next = ConversationSet.upsert(current, event.conversation);
          setUnreadCount(ConversationSet.unreadCount(next));
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
    const last = conversationsRef.current.at(-1);
    if (!last || loadingMoreRef.current) {
      return;
    }
    loadingMoreRef.current = true;
    if (!prefetch.isReady()) {
      setLoadingMore(true);
    }
    setError("");
    try {
      const page = await prefetch.takeNext(last.id);
      if (page.length === 0) {
        setHasMore(false);
        return;
      }
      const next = ConversationSet.appendPage(conversationsRef.current, page);
      setHasMore(pageHasMore(page.length));
      setConversations(next);
      prefetch.prepareNext(next, page.length);
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : "続きの読み込みに失敗しました");
    } finally {
      loadingMoreRef.current = false;
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
          const lastStatus = conversation.lastStatus;
          const preview = lastStatus
            ? Status.displayBody(lastStatus).content.replace(/<[^>]+>/g, "")
            : "";
          return (
            <button
              key={conversation.id}
              type="button"
              className={`conversation-row${Conversation.isUnread(conversation) ? " is-unread" : ""}`}
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
      <LoadMoreFooter
        hasMore={hasMore && !loading && conversations.length > 0}
        loading={loadingMore}
        observeKey={conversations.length}
        onLoadMore={() => void handleLoadMore()}
      />
    </AppShell>
  );
};
