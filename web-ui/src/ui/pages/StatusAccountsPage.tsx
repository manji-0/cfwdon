import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { AccountProfile } from "@/domain/account/account";
import {
  fetchStatusFavouritedBy,
  fetchStatusRebloggedBy,
} from "@/infrastructure/api/status";
import { AccountRow } from "@/ui/components/AccountRow";
import { AppShell } from "@/ui/components/AppShell";
import { LoadMoreFooter } from "@/ui/components/LoadMoreFooter";
import { usePagePrefetch } from "@/ui/hooks/usePagePrefetch";
import { TIMELINE_PAGE_LIMIT, pageHasMore } from "@/ui/lib/pagination";

type InteractionKind = "favourited-by" | "reblogged-by";

export const StatusAccountsPage = ({ kind }: Readonly<{ kind: InteractionKind }>) => {
  const { statusId } = useParams();
  const [accounts, setAccounts] = useState<ReadonlyArray<AccountProfile>>([]);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const [error, setError] = useState("");
  const loadingMoreRef = useRef(false);
  const accountsRef = useRef(accounts);
  accountsRef.current = accounts;
  const title = kind === "favourited-by" ? "いいねした人" : "ブーストした人";
  const fetchPage = kind === "favourited-by" ? fetchStatusFavouritedBy : fetchStatusRebloggedBy;
  const prefetch = usePagePrefetch(async (maxId: string) => {
    if (!statusId) {
      return [];
    }
    const result = await fetchPage(statusId, { maxId, limit: TIMELINE_PAGE_LIMIT });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    return result.value;
  });

  const load = useCallback(async () => {
    if (!statusId) {
      return;
    }
    const result = await fetchPage(statusId, { limit: TIMELINE_PAGE_LIMIT });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    setHasMore(pageHasMore(result.value.length));
    setAccounts(result.value);
    prefetch.prepareNext(result.value, result.value.length);
  }, [fetchPage, prefetch, statusId]);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setError("");
    void load()
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
  }, [load]);

  const handleLoadMore = async () => {
    const last = accountsRef.current.at(-1);
    if (!last || loadingMoreRef.current) {
      return;
    }
    loadingMoreRef.current = true;
    if (!prefetch.isReady()) {
      setLoadingMore(true);
    }
    try {
      const page = await prefetch.takeNext(last.id);
      if (page.length === 0) {
        setHasMore(false);
        return;
      }
      const next = [...accountsRef.current, ...page];
      setHasMore(pageHasMore(page.length));
      setAccounts(next);
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
      <p className="thread-back">
        <Link to={statusId ? `/status/${statusId}` : "/"}>← 投稿に戻る</Link>
      </p>
      {error ? <p className="app-error">{error}</p> : null}
      {loading ? <div className="app-status">読み込み中…</div> : null}
      <div className="search-accounts">
        {accounts.map((account) => (
          <AccountRow key={account.id} account={account} />
        ))}
      </div>
      {!loading && accounts.length === 0 ? (
        <div className="app-card">
          <p className="app-muted">{title}はまだいません。</p>
        </div>
      ) : null}
      <LoadMoreFooter
        hasMore={hasMore && !loading && accounts.length > 0}
        loading={loadingMore}
        observeKey={accounts.length}
        onLoadMore={() => void handleLoadMore()}
      />
    </AppShell>
  );
};

export const StatusFavouritedByPage = () => <StatusAccountsPage kind="favourited-by" />;

export const StatusRebloggedByPage = () => <StatusAccountsPage kind="reblogged-by" />;
