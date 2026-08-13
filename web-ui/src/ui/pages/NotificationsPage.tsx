import { useCallback, useEffect, useRef, useState } from "react";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import { ViewCache } from "@/domain/cache/view-cache";
import type { Notification } from "@/domain/notification/notification";
import { fetchNotifications } from "@/infrastructure/api/notification";
import { AppShell } from "@/ui/components/AppShell";
import { NotificationCard } from "@/ui/components/NotificationCard";
import { useViewCache } from "@/ui/context/ViewCacheContext";
import { useStreamingNotifications } from "@/ui/hooks/useStreamingNotifications";
import { useWindowScrollY } from "@/ui/hooks/useWindowScrollY";

export const NotificationsPage = () => {
  const cache = useViewCache();
  const cached = cache.getNotifications();
  const [notifications, setNotifications] = useState<ReadonlyArray<Notification>>(
    cached?.notifications ?? [],
  );
  const [loading, setLoading] = useState(!cached);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState("");
  const fetchedAtRef = useRef(cached?.fetchedAt ?? 0);
  const notificationsRef = useRef(notifications);
  const scrollYRef = useWindowScrollY();
  notificationsRef.current = notifications;

  useStreamingNotifications(!loading, setNotifications);

  const persist = useCallback(
    (next: ReadonlyArray<Notification>, fetchedAt: number) => {
      if (fetchedAt === 0) {
        return;
      }
      cache.writeNotifications({
        notifications: next,
        fetchedAt,
        scrollY: scrollYRef.current,
      });
    },
    [cache],
  );

  const loadNotifications = useCallback(
    async (options?: { maxId?: string; replace?: boolean }) => {
      const result = await fetchNotifications({ maxId: options?.maxId, limit: 20 });
      if (result.isErr()) {
        throw new Error(mastodonErrorMessage(result.error));
      }
      const replace = options?.replace || !options?.maxId;
      const next = replace ? result.value : [...notificationsRef.current, ...result.value];
      if (replace) {
        fetchedAtRef.current = Date.now();
      }
      setNotifications(next);
      persist(next, fetchedAtRef.current);
    },
    [persist],
  );

  useEffect(() => {
    const snapshot = cache.getNotifications();
    if (snapshot) {
      setNotifications(snapshot.notifications);
      fetchedAtRef.current = snapshot.fetchedAt;
      setLoading(false);
      requestAnimationFrame(() => window.scrollTo(0, snapshot.scrollY));
    }

    let active = true;
    if (snapshot && ViewCache.isFresh(snapshot.fetchedAt)) {
      return () => {
        persist(notificationsRef.current, fetchedAtRef.current);
      };
    }

    if (!snapshot) {
      setLoading(true);
    }
    void loadNotifications({ replace: true })
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
  }, [cache, loadNotifications, persist]);

  const handleLoadMore = async () => {
    const last = notifications.at(-1);
    if (!last || loadingMore) {
      return;
    }
    setLoadingMore(true);
    setError("");
    try {
      await loadNotifications({ maxId: last.id });
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : "続きの読み込みに失敗しました");
    } finally {
      setLoadingMore(false);
    }
  };

  return (
    <AppShell title="通知">
      {error ? <p className="app-error">{error}</p> : null}
      {loading ? <div className="app-status">読み込み中…</div> : null}
      <div className="timeline">
        {notifications.map((notification) => (
          <NotificationCard key={notification.id} notification={notification} />
        ))}
      </div>
      {!loading && notifications.length === 0 ? (
        <div className="app-card">
          <p className="app-muted">通知はまだありません。</p>
        </div>
      ) : null}
      {notifications.length > 0 ? (
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
