import { useCallback, useEffect, useState } from "react";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { AccountList } from "@/domain/lists/list";
import type { Status } from "@/domain/status/status";
import { fetchListTimeline, fetchLists } from "@/infrastructure/api/lists";
import {
  favouriteStatus,
  reblogStatus,
  unfavouriteStatus,
  unreblogStatus,
} from "@/infrastructure/api/status";
import { WebUiPhase } from "@/plan/phases";
import { AppShell } from "@/ui/components/AppShell";
import { StatusCard } from "@/ui/components/StatusCard";

export const ListsPage = () => {
  const [lists, setLists] = useState<ReadonlyArray<AccountList>>([]);
  const [selectedListId, setSelectedListId] = useState<string | null>(null);
  const [statuses, setStatuses] = useState<ReadonlyArray<Status>>([]);
  const [loadingLists, setLoadingLists] = useState(true);
  const [loadingTimeline, setLoadingTimeline] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    let active = true;
    setLoadingLists(true);
    void (async () => {
      const result = await fetchLists();
      if (!active) {
        return;
      }
      if (result.isErr()) {
        setError(mastodonErrorMessage(result.error));
      } else {
        setLists(result.value);
        setSelectedListId(result.value[0]?.id ?? null);
      }
      setLoadingLists(false);
    })();
    return () => {
      active = false;
    };
  }, []);

  const loadTimeline = useCallback(
    async (listId: string, options?: { maxId?: string; replace?: boolean }) => {
      const result = await fetchListTimeline(listId, { maxId: options?.maxId, limit: 20 });
      if (result.isErr()) {
        throw new Error(mastodonErrorMessage(result.error));
      }
      setStatuses((current) =>
        options?.replace || !options?.maxId ? result.value : [...current, ...result.value],
      );
    },
    [],
  );

  useEffect(() => {
    if (!selectedListId) {
      setStatuses([]);
      return;
    }
    let active = true;
    setLoadingTimeline(true);
    setError("");
    void loadTimeline(selectedListId, { replace: true })
      .catch((loadError) => {
        if (active) {
          setError(loadError instanceof Error ? loadError.message : "リストの読み込みに失敗しました");
        }
      })
      .finally(() => {
        if (active) {
          setLoadingTimeline(false);
        }
      });
    return () => {
      active = false;
    };
  }, [selectedListId, loadTimeline]);

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
    if (!selectedListId) {
      return;
    }
    const last = statuses.at(-1);
    if (!last || loadingMore) {
      return;
    }
    setLoadingMore(true);
    setError("");
    try {
      await loadTimeline(selectedListId, { maxId: last.id });
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : "続きの読み込みに失敗しました");
    } finally {
      setLoadingMore(false);
    }
  };

  const selectedList = lists.find((list) => list.id === selectedListId) ?? null;

  return (
    <AppShell
      title="リスト"
      aside={
        <div className="app-card" data-phase={WebUiPhase.collections}>
          <h2>リスト</h2>
          {loadingLists ? <p className="app-muted">読み込み中…</p> : null}
          {!loadingLists && lists.length === 0 ? (
            <p className="app-muted">リストはまだありません。</p>
          ) : null}
          <ul className="library-nav-list">
            {lists.map((list) => (
              <li key={list.id}>
                <button
                  type="button"
                  className={`library-nav-link${list.id === selectedListId ? " is-active" : ""}`}
                  onClick={() => setSelectedListId(list.id)}
                >
                  {list.title || "無題のリスト"}
                </button>
              </li>
            ))}
          </ul>
        </div>
      }
    >
      <div data-phase={WebUiPhase.collections}>
        {error ? <p className="app-error">{error}</p> : null}
        {selectedList ? <h2 className="library-section-title">{selectedList.title || "無題のリスト"}</h2> : null}
        {loadingTimeline ? <div className="app-status">読み込み中…</div> : null}
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
        {!loadingLists && !loadingTimeline && selectedList && statuses.length === 0 ? (
          <div className="app-card">
            <p className="app-muted">このリストにはまだ投稿がありません。</p>
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
