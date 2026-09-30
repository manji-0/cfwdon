import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import { ForegroundResume } from "@/domain/cache/foreground-resume";
import { Status } from "@/domain/status/status";
import type { MastodonFetchError } from "@/domain/errors/http-error";
import type { PageQuery } from "@/domain/pagination";
import { AppShell } from "@/ui/components/AppShell";
import { ListStatus } from "@/ui/components/ListStatus";
import { LoadMoreFooter } from "@/ui/components/LoadMoreFooter";
import { StatusCard } from "@/ui/components/StatusCard";
import { useSession } from "@/ui/context/SessionContext";
import { useForegroundCatchUp } from "@/ui/hooks/useForegroundCatchUp";
import { usePagePrefetch } from "@/ui/hooks/usePagePrefetch";
import { useStatusActions } from "@/ui/components/useStatusActions";
import { TIMELINE_PAGE_LIMIT, pageHasMore } from "@/ui/lib/pagination";
import type { ResultAsync } from "neverthrow";

type StatusCollectionPageProps = Readonly<{
  title: string;
  emptyMessage: string;
  header?: ReactNode;
  fetchPage: (query: PageQuery) => ResultAsync<ReadonlyArray<Status>, MastodonFetchError>;
}>;

export const StatusCollectionPage = ({
  title,
  emptyMessage,
  header,
  fetchPage,
}: StatusCollectionPageProps) => {
  const { session } = useSession();
  const selfAccountId = session.kind === "Authenticated" ? session.account.id : null;
  const [statuses, setStatuses] = useState<ReadonlyArray<Status>>([]);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const [error, setError] = useState("");
  const loadingMoreRef = useRef(false);
  const statusesRef = useRef(statuses);
  statusesRef.current = statuses;
  const prefetch = usePagePrefetch(async (maxId: string) => {
    const result = await fetchPage({ maxId, limit: TIMELINE_PAGE_LIMIT });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    return result.value;
  });

  const loadPage = useCallback(async () => {
    const result = await fetchPage({ limit: TIMELINE_PAGE_LIMIT });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    setHasMore(pageHasMore(result.value.length));
    setStatuses(result.value);
    prefetch.prepareNext(result.value, result.value.length);
  }, [fetchPage, prefetch]);

  const catchUpPage = useCallback(() => {
    if (!ForegroundResume.shouldRefreshVisibleList(window.scrollY)) {
      return;
    }
    setError("");
    void loadPage().catch((loadError) => {
      setError(loadError instanceof Error ? loadError.message : "読み込みに失敗しました");
    });
  }, [loadPage]);
  useForegroundCatchUp(catchUpPage);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setError("");
    void loadPage()
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
  }, [loadPage]);

  const actions = useStatusActions({
    selfAccountId,
    onReplace: (updated) => setStatuses((current) => Status.replaceInList(current, updated)),
    onRemove: (statusId) => setStatuses((current) => Status.removeById(current, statusId)),
  });

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
      prefetch.prepareNext(next, page.length);
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : "続きの読み込みに失敗しました");
    } finally {
      loadingMoreRef.current = false;
      setLoadingMore(false);
    }
  };

  return (
    <AppShell title={title}>
      {header}
      <ListStatus
        loading={loading}
        error={error}
        isEmpty={statuses.length === 0}
        empty={emptyMessage}
      />
      <div className="timeline">
        {statuses.map((status) => (
          <StatusCard key={status.id} status={status} {...actions} />
        ))}
      </div>
      <LoadMoreFooter
        hasMore={hasMore && !loading && statuses.length > 0}
        loading={loadingMore}
        observeKey={statuses.length}
        onLoadMore={() => void handleLoadMore()}
      />
    </AppShell>
  );
};
