import { useCallback, useEffect, useRef, useState } from "react";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import { CachedView } from "@/domain/cache/cached-view";
import { ForegroundResume } from "@/domain/cache/foreground-resume";
import { ViewReadiness } from "@/domain/cache/view-readiness";
import { Status } from "@/domain/status/status";
import { fetchHomeTimeline, fetchPublicTimeline } from "@/infrastructure/api/status";
import { AppShell } from "@/ui/components/AppShell";
import { LoadMoreFooter } from "@/ui/components/LoadMoreFooter";
import { StatusCard } from "@/ui/components/StatusCard";
import { useCompose } from "@/ui/context/ComposeContext";
import { useSession } from "@/ui/context/SessionContext";
import { useViewCache } from "@/ui/context/ViewCacheContext";
import { useKeyboardShortcuts } from "@/ui/hooks/useKeyboardShortcuts";
import { usePagePrefetch } from "@/ui/hooks/usePagePrefetch";
import { useStatusActions } from "@/ui/hooks/useStatusActions";
import { useForegroundCatchUp } from "@/ui/hooks/useForegroundCatchUp";
import { useStreamingTimeline } from "@/ui/hooks/useStreamingTimeline";
import { useWindowScrollY } from "@/ui/hooks/useWindowScrollY";
import { TIMELINE_PAGE_LIMIT, pageHasMore } from "@/ui/lib/pagination";

type HomeFeed = "home" | "local";

export const HomePage = () => {
  const { session } = useSession();
  const { lastPublished, lastPublishToken } = useCompose();
  const seenPublishTokenRef = useRef(lastPublishToken);
  const selfAccountId = session.kind === "Authenticated" ? session.account.id : null;
  const cache = useViewCache();
  const cached = cache.getHome();
  const [feed, setFeed] = useState<HomeFeed>("home");
  const feedRef = useRef(feed);
  feedRef.current = feed;
  const [statuses, setStatuses] = useState<ReadonlyArray<Status>>(
    cached.kind === "Present" ? cached.value.statuses : [],
  );
  const [loading, setLoading] = useState(CachedView.isAbsent(cached));
  const [loadingMore, setLoadingMore] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const [error, setError] = useState("");
  const fetchedAtRef = useRef(cached.kind === "Present" ? cached.value.fetchedAt : 0);
  const statusesRef = useRef(statuses);
  const loadingMoreRef = useRef(false);
  const scrollYRef = useWindowScrollY();
  statusesRef.current = statuses;
  const prefetch = usePagePrefetch(async (maxId: string) => {
    const result =
      feedRef.current === "local"
        ? await fetchPublicTimeline({ maxId, limit: TIMELINE_PAGE_LIMIT, local: true })
        : await fetchHomeTimeline({ maxId, limit: TIMELINE_PAGE_LIMIT });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    return result.value;
  });

  useStreamingTimeline(!loading && feed === "home", setStatuses);

  useEffect(() => {
    if (!lastPublished || lastPublishToken === seenPublishTokenRef.current) {
      return;
    }
    seenPublishTokenRef.current = lastPublishToken;
    setStatuses((current) => {
      const replaced = Status.replaceInList(current, lastPublished);
      if (!Object.is(replaced, current)) {
        return replaced;
      }
      return feedRef.current === "home" ? Status.prependUnique(current, lastPublished) : current;
    });
  }, [lastPublished, lastPublishToken]);

  const persist = useCallback(
    (nextStatuses: ReadonlyArray<Status>, fetchedAt: number) => {
      if (fetchedAt === 0 || feedRef.current !== "home") {
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

  const loadTimeline = useCallback(async () => {
    const result =
      feed === "local"
        ? await fetchPublicTimeline({ limit: TIMELINE_PAGE_LIMIT, local: true })
        : await fetchHomeTimeline({ limit: TIMELINE_PAGE_LIMIT });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    const next = result.value;
    fetchedAtRef.current = Date.now();
    setHasMore(pageHasMore(next.length));
    setStatuses(next);
    persist(next, fetchedAtRef.current);
    prefetch.prepareNext(next, next.length);
  }, [feed, persist, prefetch]);

  const catchUpTimeline = useCallback(() => {
    if (!ForegroundResume.shouldRefreshVisibleList(window.scrollY)) {
      return;
    }
    setError("");
    void loadTimeline().catch((refreshError) => {
      setError(refreshError instanceof Error ? refreshError.message : "更新に失敗しました");
    });
  }, [loadTimeline]);
  useForegroundCatchUp(catchUpTimeline);

  useEffect(() => {
    const snapshot = feed === "home" ? cache.getHome() : CachedView.absent();
    if (snapshot.kind === "Present") {
      setStatuses(snapshot.value.statuses);
      fetchedAtRef.current = snapshot.value.fetchedAt;
      setHasMore(pageHasMore(snapshot.value.statuses.length));
      setLoading(false);
      prefetch.prepareNext(snapshot.value.statuses, snapshot.value.statuses.length);
      requestAnimationFrame(() => window.scrollTo(0, snapshot.value.scrollY));
    } else {
      setStatuses([]);
      fetchedAtRef.current = 0;
      prefetch.reset();
      setLoading(true);
    }

    let active = true;
    const readiness =
      feed === "home" ? ViewReadiness.forStreaming(snapshot, Date.now()) : { kind: "Load" as const };
    switch (readiness.kind) {
      case "Skip":
        return () => {
          persist(statusesRef.current, fetchedAtRef.current);
        };
      case "Load":
        setLoading(true);
        break;
      case "Revalidate":
        break;
    }
    void loadTimeline()
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
      persist(statusesRef.current, fetchedAtRef.current);
    };
  }, [cache, feed, loadTimeline, persist, prefetch]);

  const handleRefresh = async () => {
    setError("");
    try {
      await loadTimeline();
    } catch (refreshError) {
      setError(refreshError instanceof Error ? refreshError.message : "更新に失敗しました");
    }
  };

  const handleLoadMore = async () => {
    const last = statusesRef.current.at(-1);
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
      const next = [...statusesRef.current, ...page];
      setHasMore(pageHasMore(page.length));
      setStatuses(next);
      persist(next, fetchedAtRef.current);
      prefetch.prepareNext(next, page.length);
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : "続きの読み込みに失敗しました");
    } finally {
      loadingMoreRef.current = false;
      setLoadingMore(false);
    }
  };

  const actions = useStatusActions({
    selfAccountId,
    onReplace: (updated) => {
      setStatuses((current) => Status.replaceInList(current, updated));
      cache.patchStatus(updated);
    },
    onRemove: (statusId) => {
      setStatuses((current) => Status.removeById(current, statusId));
    },
    onError: setError,
  });

  useKeyboardShortcuts([
    {
      key: "r",
      handler: () => {
        void handleRefresh();
      },
    },
  ]);

  return (
    <AppShell title="ホーム">
      <nav className="timeline-tabs" aria-label="フィード">
        <button
          type="button"
          className={feed === "home" ? "is-active" : undefined}
          onClick={() => setFeed("home")}
        >
          フォロー中
        </button>
        <button
          type="button"
          className={feed === "local" ? "is-active" : undefined}
          onClick={() => setFeed("local")}
        >
          このインスタンス
        </button>
      </nav>
      {error ? <p className="app-error">{error}</p> : null}
      {loading ? <div className="app-status">読み込み中…</div> : null}
      <div className="timeline">
        {statuses.map((status) => (
          <StatusCard key={status.id} status={status} {...actions} />
        ))}
      </div>
      {!loading && statuses.length === 0 ? (
        <div className="app-card">
          <p className="app-muted">
            {feed === "local"
              ? "このインスタンスの投稿はまだありません。"
              : "まだ投稿がありません。投稿ボタンから最初の投稿をしてみましょう。"}
          </p>
        </div>
      ) : null}
      <LoadMoreFooter
        hasMore={hasMore && !loading && statuses.length > 0}
        loading={loadingMore}
        observeKey={statuses.length}
        onLoadMore={() => void handleLoadMore()}
      />
    </AppShell>
  );
};
