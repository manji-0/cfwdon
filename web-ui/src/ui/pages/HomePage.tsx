import { useCallback, useEffect, useRef, useState } from "react";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import { ViewCache } from "@/domain/cache/view-cache";
import type { Status } from "@/domain/status/status";
import { Status as StatusModel } from "@/domain/status/status";
import {
  bookmarkStatus,
  createStatus,
  favouriteStatus,
  fetchHomeTimeline,
  reblogStatus,
  unbookmarkStatus,
  unfavouriteStatus,
  unreblogStatus,
} from "@/infrastructure/api/status";
import { Visibility } from "@/domain/status/visibility";
import { Composer, type ComposerHandle } from "@/ui/components/Composer";
import { AppShell } from "@/ui/components/AppShell";
import { useKeyboardShortcuts } from "@/ui/hooks/useKeyboardShortcuts";
import { useStreamingTimeline } from "@/ui/hooks/useStreamingTimeline";
import { useWindowScrollY } from "@/ui/hooks/useWindowScrollY";
import { StatusCard } from "@/ui/components/StatusCard";
import { TrendsSidebar } from "@/ui/components/TrendsSidebar";
import { useViewCache } from "@/ui/context/ViewCacheContext";

export const HomePage = () => {
  const composerRef = useRef<ComposerHandle>(null);
  const cache = useViewCache();
  const cached = cache.getHome();
  const [statuses, setStatuses] = useState<ReadonlyArray<Status>>(cached?.statuses ?? []);
  const [loading, setLoading] = useState(!cached);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState("");
  const [refreshing, setRefreshing] = useState(false);
  const fetchedAtRef = useRef(cached?.fetchedAt ?? 0);
  const statusesRef = useRef(statuses);
  const scrollYRef = useWindowScrollY();
  statusesRef.current = statuses;

  useStreamingTimeline(!loading, setStatuses);

  const persist = useCallback(
    (nextStatuses: ReadonlyArray<Status>, fetchedAt: number) => {
      if (fetchedAt === 0) {
        return;
      }
      cache.writeHome({
        statuses: nextStatuses,
        fetchedAt,
        scrollY: scrollYRef.current,
      });
    },
    [cache],
  );

  const loadTimeline = useCallback(
    async (options?: { maxId?: string; replace?: boolean }) => {
      const result = await fetchHomeTimeline({ maxId: options?.maxId, limit: 20 });
      if (result.isErr()) {
        throw new Error(mastodonErrorMessage(result.error));
      }
      const replace = options?.replace || !options?.maxId;
      const next = replace ? result.value : [...statusesRef.current, ...result.value];
      if (replace) {
        fetchedAtRef.current = Date.now();
      }
      setStatuses(next);
      persist(next, fetchedAtRef.current);
    },
    [persist],
  );

  useEffect(() => {
    const snapshot = cache.getHome();
    if (snapshot) {
      setStatuses(snapshot.statuses);
      fetchedAtRef.current = snapshot.fetchedAt;
      setLoading(false);
      requestAnimationFrame(() => window.scrollTo(0, snapshot.scrollY));
    }

    let active = true;
    if (snapshot && ViewCache.isRemountSkip(snapshot.fetchedAt)) {
      return () => {
        persist(statusesRef.current, fetchedAtRef.current);
      };
    }

    if (!snapshot) {
      setLoading(true);
    } else {
      setRefreshing(true);
    }
    void loadTimeline({ replace: true })
      .catch((loadError) => {
        if (active) {
          setError(loadError instanceof Error ? loadError.message : "タイムラインの読み込みに失敗しました");
        }
      })
      .finally(() => {
        if (active) {
          setLoading(false);
          setRefreshing(false);
        }
      });
    return () => {
      active = false;
      persist(statusesRef.current, fetchedAtRef.current);
    };
  }, [cache, loadTimeline, persist]);

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
    setStatuses((current) => {
      const next = StatusModel.prependUnique(current, result.value);
      persist(next, fetchedAtRef.current);
      return next;
    });
  };

  const updateStatusInList = (updated: Status) => {
    setStatuses((current) => StatusModel.replaceInList(current, updated));
    cache.patchStatus(updated);
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

  const handleBookmark = async (status: Status) => {
    const result = status.bookmarked
      ? await unbookmarkStatus(status.id)
      : await bookmarkStatus(status.id);
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    updateStatusInList(result.value);
  };

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
            onBookmark={(body) => void handleBookmark(body)}
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
