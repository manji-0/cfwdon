import { useCallback, useEffect, useRef, useState } from "react";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { Status } from "@/domain/status/status";
import {
  createStatus,
  favouriteStatus,
  fetchHomeTimeline,
  reblogStatus,
  unfavouriteStatus,
  unreblogStatus,
} from "@/infrastructure/api/status";
import { Visibility } from "@/domain/status/visibility";
import { Composer, type ComposerHandle } from "@/ui/components/Composer";
import { AppShell } from "@/ui/components/AppShell";
import { useKeyboardShortcuts } from "@/ui/hooks/useKeyboardShortcuts";
import { useStreamingTimeline } from "@/ui/hooks/useStreamingTimeline";
import { StatusCard } from "@/ui/components/StatusCard";
import { TrendsSidebar } from "@/ui/components/TrendsSidebar";

export const HomePage = () => {
  const composerRef = useRef<ComposerHandle>(null);
  const [statuses, setStatuses] = useState<ReadonlyArray<Status>>([]);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState("");
  const [refreshing, setRefreshing] = useState(false);

  useStreamingTimeline(!loading, setStatuses);

  const loadTimeline = useCallback(async (options?: { maxId?: string; replace?: boolean }) => {
    const result = await fetchHomeTimeline({ maxId: options?.maxId, limit: 20 });
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
    void loadTimeline({ replace: true })
      .catch((loadError) => {
        if (active) {
          setError(loadError instanceof Error ? loadError.message : "タイムラインの読み込みに失敗しました");
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
  }, [loadTimeline]);

  const handleRefresh = async () => {
    setRefreshing(true);
    setError("");
    try {
      await loadTimeline({ replace: true });
    } catch (refreshError) {
      setError(refreshError instanceof Error ? refreshError.message : "更新に失敗しました");
    } finally {
      setRefreshing(false);
    }
  };

  const handleLoadMore = async () => {
    const last = statuses.at(-1);
    if (!last || loadingMore) {
      return;
    }
    setLoadingMore(true);
    setError("");
    try {
      await loadTimeline({ maxId: last.id });
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : "続きの読み込みに失敗しました");
    } finally {
      setLoadingMore(false);
    }
  };

  const handlePublish = async (input: {
    text: string;
    visibility: ReturnType<typeof Visibility.public>;
    spoilerText: string;
    sensitive: boolean;
    mediaIds: ReadonlyArray<string>;
  }) => {
    const result = await createStatus({
      text: input.text,
      visibility: Visibility.toApi(input.visibility),
      spoilerText: input.spoilerText,
      sensitive: input.sensitive,
      mediaIds: input.mediaIds,
    });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    setStatuses((current) => [result.value, ...current]);
  };

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

  useKeyboardShortcuts([
    {
      key: "n",
      handler: () => composerRef.current?.focus(),
    },
    {
      key: "r",
      handler: () => {
        void handleRefresh();
      },
    },
  ]);

  const handleReblog = async (status: Status) => {
    const targetId = status.id;
    const result = status.reblogged
      ? await unreblogStatus(targetId)
      : await reblogStatus(targetId);
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    await handleRefresh();
  };

  return (
    <AppShell
      title="ホーム"
      aside={
        <>
          <div className="app-card">
            <h2>ホーム</h2>
            <p className="app-muted">フォロー中のアカウントの投稿が表示されます。</p>
            <button type="button" className="app-button app-button-secondary" onClick={() => void handleRefresh()}>
              {refreshing ? "更新中…" : "更新"}
            </button>
          </div>
          <TrendsSidebar />
        </>
      }
    >
      <Composer ref={composerRef} onSubmit={handlePublish} />
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
          <p className="app-muted">まだ投稿がありません。最初の投稿をしてみましょう。</p>
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
    </AppShell>
  );
};
