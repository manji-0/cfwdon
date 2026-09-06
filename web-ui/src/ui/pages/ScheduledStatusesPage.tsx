import { useCallback, useEffect, useRef, useState } from "react";
import { Link } from "react-router";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { ScheduledStatus } from "@/domain/status/scheduled";
import { AppRoute } from "@/domain/navigation/route";
import {
  cancelScheduledStatus,
  fetchScheduledStatuses,
} from "@/infrastructure/api/scheduled";
import { AppShell } from "@/ui/components/AppShell";
import { LoadMoreFooter } from "@/ui/components/LoadMoreFooter";
import { useConfirm } from "@/ui/context/ConfirmContext";
import { formatDateTime } from "@/ui/lib/time";
import { TIMELINE_PAGE_LIMIT, pageHasMore } from "@/ui/lib/pagination";

export const ScheduledStatusesPage = () => {
  const { confirm } = useConfirm();
  const [items, setItems] = useState<ReadonlyArray<ScheduledStatus>>([]);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const [error, setError] = useState("");
  const loadingMoreRef = useRef(false);
  const itemsRef = useRef(items);
  itemsRef.current = items;

  const load = useCallback(async () => {
    const result = await fetchScheduledStatuses({ limit: TIMELINE_PAGE_LIMIT });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    setHasMore(pageHasMore(result.value.length));
    setItems(result.value);
  }, []);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setError("");
    void load()
      .catch((loadError) => {
        if (active) {
          setError(loadError instanceof Error ? loadError.message : "読み込みに失敗しました");
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
  }, [load]);

  const handleLoadMore = async () => {
    const last = itemsRef.current.at(-1);
    if (!last || loadingMoreRef.current) {
      return;
    }
    loadingMoreRef.current = true;
    setLoadingMore(true);
    try {
      const result = await fetchScheduledStatuses({
        maxId: last.id,
        limit: TIMELINE_PAGE_LIMIT,
      });
      if (result.isErr()) {
        throw new Error(mastodonErrorMessage(result.error));
      }
      if (result.value.length === 0) {
        setHasMore(false);
        return;
      }
      setHasMore(pageHasMore(result.value.length));
      setItems((current) => [...current, ...result.value]);
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : "続きの読み込みに失敗しました");
    } finally {
      loadingMoreRef.current = false;
      setLoadingMore(false);
    }
  };

  const handleCancel = async (id: string) => {
    const ok = await confirm("この予約投稿を取り消しますか？", {
      title: "予約の取り消し",
      confirmLabel: "取り消す",
      danger: true,
    });
    if (!ok) {
      return;
    }
    const result = await cancelScheduledStatus(id);
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    setItems((current) => current.filter((item) => item.id !== id));
  };

  return (
    <AppShell title="予約投稿">
      <p className="thread-back">
        <Link to={AppRoute.toPath(AppRoute.home())}>← ホーム</Link>
      </p>
      {error ? <p className="app-error">{error}</p> : null}
      {loading ? <div className="app-status">読み込み中…</div> : null}
      <div className="timeline">
        {items.map((item) => (
          <article key={item.id} className="app-card scheduled-status-card">
            <p className="app-muted">{formatDateTime(item.scheduledAt)}</p>
            {item.spoilerText ? <p>CW: {item.spoilerText}</p> : null}
            <p>{item.text || "（本文なし）"}</p>
            <button
              type="button"
              className="app-button app-button-secondary"
              onClick={() => void handleCancel(item.id)}
            >
              取り消す
            </button>
          </article>
        ))}
      </div>
      {!loading && items.length === 0 ? (
        <div className="app-card">
          <p className="app-muted">予約投稿はありません。</p>
        </div>
      ) : null}
      <LoadMoreFooter
        hasMore={hasMore && !loading && items.length > 0}
        loading={loadingMore}
        observeKey={items.length}
        onLoadMore={() => void handleLoadMore()}
      />
    </AppShell>
  );
};
