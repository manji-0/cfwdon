import { useCallback, useEffect, useRef, useState } from "react";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import { CachedView } from "@/domain/cache/cached-view";
import { ViewReadiness } from "@/domain/cache/view-readiness";
import {
  NotificationFilter,
  type NotificationFilterType,
} from "@/domain/notification/filter";
import type { Notification } from "@/domain/notification/notification";
import {
  clearNotifications,
  dismissNotification,
  fetchNotifications,
} from "@/infrastructure/api/notification";
import {
  authorizeFollowRequest,
  rejectFollowRequest,
} from "@/infrastructure/api/relationship";
import { AppShell } from "@/ui/components/AppShell";
import { LoadMoreFooter } from "@/ui/components/LoadMoreFooter";
import { NotificationCard } from "@/ui/components/NotificationCard";
import { useConfirm } from "@/ui/context/ConfirmContext";
import { useUnreadNotifications } from "@/ui/context/UnreadNotificationsContext";
import { useViewCache } from "@/ui/context/ViewCacheContext";
import { useStreamingNotifications } from "@/ui/hooks/useStreamingNotifications";
import { usePagePrefetch } from "@/ui/hooks/usePagePrefetch";
import { useWindowScrollY } from "@/ui/hooks/useWindowScrollY";
import { TIMELINE_PAGE_LIMIT, pageHasMore } from "@/ui/lib/pagination";

export const NotificationsPage = () => {
  const cache = useViewCache();
  const cached = cache.getNotifications();
  const { confirm } = useConfirm();
  const { refreshUnreadCount, clearUnreadCount } = useUnreadNotifications();
  const [notifications, setNotifications] = useState<ReadonlyArray<Notification>>(
    cached.kind === "Present" ? cached.value.notifications : [],
  );
  const [filter, setFilter] = useState<NotificationFilterType | "all">("all");
  const [loading, setLoading] = useState(CachedView.isAbsent(cached));
  const [loadingMore, setLoadingMore] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const [error, setError] = useState("");
  const fetchedAtRef = useRef(cached.kind === "Present" ? cached.value.fetchedAt : 0);
  const notificationsRef = useRef(notifications);
  const loadingMoreRef = useRef(false);
  const scrollYRef = useWindowScrollY();
  notificationsRef.current = notifications;
  const prefetch = usePagePrefetch(async (maxId: string) => {
    const result = await fetchNotifications({
      maxId,
      limit: TIMELINE_PAGE_LIMIT,
      types: filter === "all" ? undefined : [filter],
    });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    return result.value;
  });

  useStreamingNotifications(!loading && filter === "all", setNotifications);

  const persist = useCallback(
    (next: ReadonlyArray<Notification>, fetchedAt: number) => {
      if (fetchedAt === 0 || filter !== "all") {
        return;
      }
      cache.writeNotifications({
        notifications: next,
        fetchedAt,
        scrollY: scrollYRef.current,
      });
    },
    [cache, filter],
  );

  const loadNotifications = useCallback(
    async () => {
      const result = await fetchNotifications({
        limit: TIMELINE_PAGE_LIMIT,
        types: filter === "all" ? undefined : [filter],
      });
      if (result.isErr()) {
        throw new Error(mastodonErrorMessage(result.error));
      }
      const next = result.value;
      fetchedAtRef.current = Date.now();
      setHasMore(pageHasMore(next.length));
      setNotifications(next);
      persist(next, fetchedAtRef.current);
      prefetch.prepareNext(next, next.length);
    },
    [filter, persist, prefetch],
  );

  useEffect(() => {
    const snapshot = cache.getNotifications();
    if (filter === "all" && snapshot.kind === "Present") {
      setNotifications(snapshot.value.notifications);
      fetchedAtRef.current = snapshot.value.fetchedAt;
      setHasMore(pageHasMore(snapshot.value.notifications.length));
      setLoading(false);
      prefetch.prepareNext(snapshot.value.notifications, snapshot.value.notifications.length);
      requestAnimationFrame(() => window.scrollTo(0, snapshot.value.scrollY));
    } else {
      setNotifications([]);
      prefetch.reset();
      setLoading(true);
    }

    let active = true;
    const readiness =
      filter === "all" ? ViewReadiness.forStreaming(snapshot, Date.now()) : { kind: "Load" as const };
    switch (readiness.kind) {
      case "Skip":
        return () => {
          persist(notificationsRef.current, fetchedAtRef.current);
        };
      case "Load":
        setLoading(true);
        break;
      case "Revalidate":
        break;
    }
    void loadNotifications()
      .catch((loadError) => {
        if (active) {
          setError(loadError instanceof Error ? loadError.message : "通知の読み込みに失敗しました");
        }
      })
      .finally(() => {
        if (active) {
          setLoading(false);
        }
      });
    return () => {
      active = false;
      persist(notificationsRef.current, fetchedAtRef.current);
    };
  }, [cache, filter, loadNotifications, persist, prefetch]);

  const handleLoadMore = async () => {
    const last = notificationsRef.current.at(-1);
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
      const next = [...notificationsRef.current, ...page];
      setHasMore(pageHasMore(page.length));
      setNotifications(next);
      persist(next, fetchedAtRef.current);
      prefetch.prepareNext(next, page.length);
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : "続きの読み込みに失敗しました");
    } finally {
      loadingMoreRef.current = false;
      setLoadingMore(false);
    }
  };

  const handleFollowRequest = async (accountId: string, accept: boolean) => {
    const result = accept
      ? await authorizeFollowRequest(accountId)
      : await rejectFollowRequest(accountId);
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    setNotifications((current) =>
      current.filter(
        (notification) =>
          !(notification.kind === "FollowRequest" && notification.account.id === accountId),
      ),
    );
  };

  const handleDismiss = async (notificationId: string) => {
    const result = await dismissNotification(notificationId);
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    setNotifications((current) => current.filter((notification) => notification.id !== notificationId));
    refreshUnreadCount();
  };

  const handleClear = async () => {
    const ok = await confirm("通知をすべて既読にして消しますか？", {
      title: "通知をクリア",
      confirmLabel: "クリア",
      danger: true,
    });
    if (!ok) {
      return;
    }
    const result = await clearNotifications();
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    setNotifications([]);
    prefetch.reset();
    setHasMore(false);
    clearUnreadCount();
  };

  return (
    <AppShell title="通知">
      <div className="notification-toolbar">
        <nav className="timeline-tabs" aria-label="通知の種類">
          <button
            type="button"
            className={filter === "all" ? "is-active" : undefined}
            onClick={() => setFilter("all")}
          >
            すべて
          </button>
          {NotificationFilter.values.map((typeName) => (
            <button
              key={typeName}
              type="button"
              className={filter === typeName ? "is-active" : undefined}
              onClick={() => setFilter(typeName)}
            >
              {NotificationFilter.labels[typeName]}
            </button>
          ))}
        </nav>
        <button
          type="button"
          className="app-button app-button-secondary"
          onClick={() => void handleClear()}
          disabled={notifications.length === 0}
        >
          すべて既読
        </button>
      </div>
      {error ? <p className="app-error">{error}</p> : null}
      {loading ? <div className="app-status">読み込み中…</div> : null}
      <div className="timeline">
        {notifications.map((notification) => (
          <NotificationCard
            key={notification.id}
            notification={notification}
            onAuthorizeFollow={(accountId) => void handleFollowRequest(accountId, true)}
            onRejectFollow={(accountId) => void handleFollowRequest(accountId, false)}
            onDismiss={(notificationId) => void handleDismiss(notificationId)}
          />
        ))}
      </div>
      {!loading && notifications.length === 0 ? (
        <div className="app-card">
          <p className="app-muted">通知はまだありません。</p>
        </div>
      ) : null}
      <LoadMoreFooter
        hasMore={hasMore && !loading && notifications.length > 0}
        loading={loadingMore}
        observeKey={notifications.length}
        onLoadMore={() => void handleLoadMore()}
      />
    </AppShell>
  );
};
