import { useCallback, useEffect, useState } from "react";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { Notification } from "@/domain/notification/notification";
import { fetchNotifications } from "@/infrastructure/api/notification";
import { AppShell } from "@/ui/components/AppShell";
import { NotificationCard } from "@/ui/components/NotificationCard";
import { useStreamingNotifications } from "@/ui/hooks/useStreamingNotifications";

export const NotificationsPage = () => {
  const [notifications, setNotifications] = useState<ReadonlyArray<Notification>>([]);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState("");

  useStreamingNotifications(!loading, setNotifications);

  const loadNotifications = useCallback(async (options?: { maxId?: string; replace?: boolean }) => {
    const result = await fetchNotifications({ maxId: options?.maxId, limit: 20 });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    setNotifications((current) =>
      options?.replace || !options?.maxId ? result.value : [...current, ...result.value],
    );
  }, []);

  useEffect(() => {
    let active = true;
    setLoading(true);
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
    };
  }, [loadNotifications]);

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
