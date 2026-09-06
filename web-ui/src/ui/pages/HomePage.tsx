import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useSearchParams } from "react-router";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import { CachedView } from "@/domain/cache/cached-view";
import { ViewReadiness } from "@/domain/cache/view-readiness";
import type { QuotedStatusPreview } from "@/domain/status/quote";
import { Status } from "@/domain/status/status";
import {
  createStatus,
  fetchHomeTimeline,
  fetchStatus,
  fetchStatusSource,
  updateStatus,
} from "@/infrastructure/api/status";
import { createScheduledStatus } from "@/infrastructure/api/scheduled";
import { Visibility } from "@/domain/status/visibility";
import { Composer, type ComposerHandle, type ComposerSubmitInput } from "@/ui/components/Composer";
import { AppShell } from "@/ui/components/AppShell";
import { LoadMoreFooter } from "@/ui/components/LoadMoreFooter";
import { SearchSidebar } from "@/ui/components/SearchSidebar";
import { StatusCard } from "@/ui/components/StatusCard";
import { TimelineTabs } from "@/ui/components/TimelineTabs";
import { TrendsSidebar } from "@/ui/components/TrendsSidebar";
import { useSession } from "@/ui/context/SessionContext";
import { useViewCache } from "@/ui/context/ViewCacheContext";
import { useKeyboardShortcuts } from "@/ui/hooks/useKeyboardShortcuts";
import { usePagePrefetch } from "@/ui/hooks/usePagePrefetch";
import { useStatusActions } from "@/ui/hooks/useStatusActions";
import { useStreamingTimeline } from "@/ui/hooks/useStreamingTimeline";
import { useWindowScrollY } from "@/ui/hooks/useWindowScrollY";
import { TIMELINE_PAGE_LIMIT, pageHasMore } from "@/ui/lib/pagination";

export const HomePage = () => {
  const composerRef = useRef<ComposerHandle>(null);
  const [searchParams, setSearchParams] = useSearchParams();
  const quoteId = searchParams.get("quote");
  const editId = searchParams.get("edit");
  const { session } = useSession();
  const selfAccountId = session.kind === "Authenticated" ? session.account.id : null;
  const cache = useViewCache();
  const cached = cache.getHome();
  const [statuses, setStatuses] = useState<ReadonlyArray<Status>>(
    cached.kind === "Present" ? cached.value.statuses : [],
  );
  const [loading, setLoading] = useState(CachedView.isAbsent(cached));
  const [loadingMore, setLoadingMore] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const [error, setError] = useState("");
  const [scheduledNotice, setScheduledNotice] = useState(false);
  const [quotedPreview, setQuotedPreview] = useState<QuotedStatusPreview | null>(null);
  const [editText, setEditText] = useState("");
  const [editSpoiler, setEditSpoiler] = useState("");
  const fetchedAtRef = useRef(cached.kind === "Present" ? cached.value.fetchedAt : 0);
  const statusesRef = useRef(statuses);
  const loadingMoreRef = useRef(false);
  const scrollYRef = useWindowScrollY();
  statusesRef.current = statuses;
  const prefetch = usePagePrefetch(async (maxId: string) => {
    const result = await fetchHomeTimeline({ maxId, limit: TIMELINE_PAGE_LIMIT });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    return result.value;
  });

  useStreamingTimeline(!loading, setStatuses);

  useEffect(() => {
    if (!quoteId) {
      setQuotedPreview(null);
      return;
    }
    let active = true;
    void fetchStatus(quoteId).then((result) => {
      if (!active || result.isErr()) {
        return;
      }
      const body = Status.displayBody(result.value);
      setQuotedPreview({
        id: body.id,
        content: body.content,
        spoilerText: body.spoilerText,
        account: body.account,
      });
    });
    return () => {
      active = false;
    };
  }, [quoteId]);

  useEffect(() => {
    if (!editId) {
      setEditText("");
      setEditSpoiler("");
      return;
    }
    let active = true;
    void fetchStatusSource(editId).then((result) => {
      if (!active || result.isErr()) {
        return;
      }
      setEditText(result.value.text);
      setEditSpoiler(result.value.spoilerText);
    });
    return () => {
      active = false;
    };
  }, [editId]);

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

  const loadTimeline = useCallback(async () => {
    const result = await fetchHomeTimeline({ limit: TIMELINE_PAGE_LIMIT });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    const next = result.value;
    fetchedAtRef.current = Date.now();
    setHasMore(pageHasMore(next.length));
    setStatuses(next);
    persist(next, fetchedAtRef.current);
    prefetch.prepareNext(next, next.length);
  }, [persist, prefetch]);

  useEffect(() => {
    const snapshot = cache.getHome();
    if (snapshot.kind === "Present") {
      setStatuses(snapshot.value.statuses);
      fetchedAtRef.current = snapshot.value.fetchedAt;
      setHasMore(pageHasMore(snapshot.value.statuses.length));
      setLoading(false);
      prefetch.prepareNext(snapshot.value.statuses, snapshot.value.statuses.length);
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
  }, [cache, loadTimeline, persist, prefetch]);

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

  const handlePublish = async (input: ComposerSubmitInput) => {
    if (!editId && input.scheduledAt) {
      const result = await createScheduledStatus({
        text: input.text,
        visibility: Visibility.toApi(input.visibility),
        scheduledAt: input.scheduledAt,
        spoilerText: input.spoilerText,
        sensitive: input.sensitive,
        language: input.language,
        mediaIds: input.mediaIds,
        poll: input.poll,
        quotedStatusId: quoteId ?? input.quotedStatusId,
      });
      if (result.isErr()) {
        throw new Error(mastodonErrorMessage(result.error));
      }
      setSearchParams({});
      setScheduledNotice(true);
      return;
    }
    const result = editId
      ? await updateStatus(editId, {
          text: input.text,
          spoilerText: input.spoilerText,
          sensitive: input.sensitive,
          mediaIds: input.mediaIds,
        })
      : await createStatus({
          text: input.text,
          visibility: Visibility.toApi(input.visibility),
          spoilerText: input.spoilerText,
          sensitive: input.sensitive,
          language: input.language,
          mediaIds: input.mediaIds,
          poll: input.poll,
          quotedStatusId: quoteId ?? input.quotedStatusId,
        });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    setSearchParams({});
    setScheduledNotice(false);
    setStatuses((current) => {
      const next = editId
        ? Status.replaceInList(current, result.value)
        : Status.prependUnique(current, result.value);
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
      <Composer
        key={editId ?? quoteId ?? "compose"}
        ref={composerRef}
        submitLabel={editId ? "保存" : "投稿"}
        applyPostingDefaults={!editId}
        initialText={editText}
        initialSpoilerText={editSpoiler}
        quotedStatusId={quoteId ?? undefined}
        quotedPreview={quotedPreview}
        allowSchedule={!editId}
        onCancel={quoteId || editId ? () => setSearchParams({}) : undefined}
        onSubmit={handlePublish}
      />
      {scheduledNotice ? (
        <p className="app-muted">
          予約しました。<Link to="/scheduled">予約投稿を見る</Link>
        </p>
      ) : null}
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
      <LoadMoreFooter
        hasMore={hasMore && !loading && statuses.length > 0}
        loading={loadingMore}
        observeKey={statuses.length}
        onLoadMore={() => void handleLoadMore()}
      />
    </AppShell>
  );
};
