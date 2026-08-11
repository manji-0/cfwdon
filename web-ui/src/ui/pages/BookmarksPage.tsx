import { useCallback, useEffect, useState } from "react";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { Status } from "@/domain/status/status";
import { fetchBookmarks } from "@/infrastructure/api/bookmarks";
import {
  favouriteStatus,
  reblogStatus,
  unfavouriteStatus,
  unreblogStatus,
} from "@/infrastructure/api/status";
import { WebUiPhase } from "@/plan/phases";
import { AppShell } from "@/ui/components/AppShell";
import { StatusCard } from "@/ui/components/StatusCard";

export const BookmarksPage = () => {
  const [statuses, setStatuses] = useState<ReadonlyArray<Status>>([]);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState("");

  const loadBookmarks = useCallback(async (options?: { maxId?: string; replace?: boolean }) => {
    const result = await fetchBookmarks({ maxId: options?.maxId, limit: 20 });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    setStatuses((current) =>
      options?.replace || !options?.maxId ? result.value : [...current, ...result.value],
    );
  }, []);

  useEffect(() => {
    let active = true;
    setLoading(true);
    void loadBookmarks({ replace: true })
      .catch((loadError) => {
        if (active) {
          setError(loadError instanceof Error ? loadError.message : "ブックマークの読み込みに失敗しました");
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
  }, [loadBookmarks]);

  const updateStatusInList = (updated: Status) => {
    setStatuses((current) =>
      current.map((item) => {
        const body = item.reblog ?? item;
        if (body.id === updated.id) {
          return item.reblog ? { ...item, reblog: updated } : updated;
        }
        return item;
      }),
    );
  };

  const handleFavourite = async (status: Status) => {
    const result = status.favourited
      ? await unfavouriteStatus(status.id)
      : await favouriteStatus(status.id);
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    updateStatusInList(result.value);
  };

  const handleReblog = async (status: Status) => {
    const result = status.reblogged
      ? await unreblogStatus(status.id)
      : await reblogStatus(status.id);
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    updateStatusInList(result.value);
  };

  const handleLoadMore = async () => {
    const last = statuses.at(-1);
    if (!last || loadingMore) {
      return;
    }
    setLoadingMore(true);
    setError("");
    try {
      await loadBookmarks({ maxId: last.id });
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : "続きの読み込みに失敗しました");
    } finally {
      setLoadingMore(false);
    }
  };

  return (
    <AppShell title="ブックマーク">
      <div data-phase={WebUiPhase.collections}>
        {error ? <p className="app-error">{error}</p> : null}
        {loading ? <div className="app-status">読み込み中…</div> : null}
        <div className="timeline">
          {statuses.map((status) => (
            <StatusCard
              key={status.id}
              status={status}
              onFavourite={(body) => void handleFavourite(body)}
              onReblog={(body) => void handleReblog(body)}
            />
          ))}
        </div>
        {!loading && statuses.length === 0 ? (
          <div className="app-card">
            <p className="app-muted">ブックマークはまだありません。</p>
          </div>
        ) : null}
        {statuses.length > 0 ? (
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
