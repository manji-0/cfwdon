import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import { Status } from "@/domain/status/status";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import type { TimelineQuery } from "@/infrastructure/api/status";
import { AppShell } from "@/ui/components/AppShell";
import { LoadMoreFooter } from "@/ui/components/LoadMoreFooter";
import { StatusCard } from "@/ui/components/StatusCard";
import { useSession } from "@/ui/context/SessionContext";
import { usePagePrefetch } from "@/ui/hooks/usePagePrefetch";
import { useStatusActions } from "@/ui/hooks/useStatusActions";
import { TIMELINE_PAGE_LIMIT, pageHasMore } from "@/ui/lib/pagination";
import type { ResultAsync } from "neverthrow";

type StatusCollectionPageProps = Readonly<{
  title: string;
  emptyMessage: string;
  header?: ReactNode;
  aside?: ReactNode;
  fetchPage: (query: TimelineQuery) => ResultAsync<ReadonlyArray<Status>, MastodonFetchError>;
}>;

export const StatusCollectionPage = ({
  title,
  emptyMessage,
  header,
  aside,
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
    onError: setError,
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
    <AppShell title={title} aside={aside}>
      {header}
      {error ? <p className="app-error">{error}</p> : null}
      {loading ? <div className="app-status">読み込み中…</div> : null}
      <div className="timeline">
        {statuses.map((status) => (
          <StatusCard key={status.id} status={status} {...actions} />
        ))}
      </div>
      {!loading && statuses.length === 0 ? (
        <div className="app-card">
          <p className="app-muted">{emptyMessage}</p>
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
