import { useCallback, useEffect, useState, type ReactNode } from "react";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import { Status } from "@/domain/status/status";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import type { TimelineQuery } from "@/infrastructure/api/status";
import { AppShell } from "@/ui/components/AppShell";
import { StatusCard } from "@/ui/components/StatusCard";
import { useSession } from "@/ui/context/SessionContext";
import { useStatusActions } from "@/ui/hooks/useStatusActions";
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
  const [error, setError] = useState("");

  const loadPage = useCallback(
    async (options?: { maxId?: string; replace?: boolean }) => {
      const result = await fetchPage({ maxId: options?.maxId, limit: 20 });
      if (result.isErr()) {
        throw new Error(mastodonErrorMessage(result.error));
      }
      setStatuses((current) =>
        options?.replace || !options?.maxId ? result.value : [...current, ...result.value],
      );
    },
    [fetchPage],
  );

  useEffect(() => {
    let active = true;
    setLoading(true);
    setError("");
    void loadPage({ replace: true })
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
    const last = statuses.at(-1);
    if (!last || loadingMore) {
      return;
    }
    setLoadingMore(true);
    setError("");
    try {
      await loadPage({ maxId: last.id });
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : "続きの読み込みに失敗しました");
    } finally {
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
