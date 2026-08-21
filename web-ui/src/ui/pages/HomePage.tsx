import { useCallback, useEffect, useRef, useState } from "react";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import { CachedView } from "@/domain/cache/cached-view";
import { ViewReadiness } from "@/domain/cache/view-readiness";
import { Status } from "@/domain/status/status";
import { createStatus, fetchHomeTimeline } from "@/infrastructure/api/status";
import { Visibility } from "@/domain/status/visibility";
import { Composer, type ComposerHandle, type ComposerSubmitInput } from "@/ui/components/Composer";
import { AppShell } from "@/ui/components/AppShell";
import { useKeyboardShortcuts } from "@/ui/hooks/useKeyboardShortcuts";
import { useStatusActions } from "@/ui/hooks/useStatusActions";
import { useStreamingTimeline } from "@/ui/hooks/useStreamingTimeline";
import { useWindowScrollY } from "@/ui/hooks/useWindowScrollY";
import { SearchSidebar } from "@/ui/components/SearchSidebar";
import { StatusCard } from "@/ui/components/StatusCard";
import { TimelineTabs } from "@/ui/components/TimelineTabs";
import { TrendsSidebar } from "@/ui/components/TrendsSidebar";
import { useSession } from "@/ui/context/SessionContext";
import { useViewCache } from "@/ui/context/ViewCacheContext";

export const HomePage = () => {
  const composerRef = useRef<ComposerHandle>(null);
  const { session } = useSession();
  const selfAccountId = session.kind === "Authenticated" ? session.account.id : null;
  const cache = useViewCache();
  const cached = cache.getHome();
  const [statuses, setStatuses] = useState<ReadonlyArray<Status>>(
    cached.kind === "Present" ? cached.value.statuses : [],
  );
  const [loading, setLoading] = useState(CachedView.isAbsent(cached));
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState("");
  const fetchedAtRef = useRef(cached.kind === "Present" ? cached.value.fetchedAt : 0);
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
    if (snapshot.kind === "Present") {
      setStatuses(snapshot.value.statuses);
      fetchedAtRef.current = snapshot.value.fetchedAt;
      setLoading(false);
      requestAnimationFrame(() => window.scrollTo(0, snapshot.value.scrollY));
    }

    let active = true;
    const readiness = ViewReadiness.forStreaming(snapshot, Date.now());
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
      persist(statusesRef.current, fetchedAtRef.current);
    };
  }, [cache, loadTimeline, persist]);

  const handleRefresh = async () => {
    setError("");
    try {
      await loadTimeline({ replace: true });
    } catch (refreshError) {
      setError(refreshError instanceof Error ? refreshError.message : "更新に失敗しました");
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

  const handlePublish = async (input: ComposerSubmitInput) => {
    const result = await createStatus({
      text: input.text,
      visibility: Visibility.toApi(input.visibility),
      spoilerText: input.spoilerText,
      sensitive: input.sensitive,
      mediaIds: input.mediaIds,
      poll: input.poll,
    });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    setStatuses((current) => {
      const next = Status.prependUnique(current, result.value);
      persist(next, fetchedAtRef.current);
      return next;
    });
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

  return (
    <AppShell
      title="ホーム"
      aside={
        <>
          <SearchSidebar />
          <TrendsSidebar />
        </>
      }
    >
      <TimelineTabs />
      <Composer ref={composerRef} onSubmit={handlePublish} />
      {error ? <p className="app-error">{error}</p> : null}
      {loading ? <div className="app-status">読み込み中…</div> : null}
      <div className="timeline">
        {statuses.map((status) => (
          <StatusCard key={status.id} status={status} {...actions} />
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
