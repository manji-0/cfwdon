import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useParams } from "react-router";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { AccountProfile } from "@/domain/account/account";
import {
  fetchAccountFollowers,
  fetchAccountFollowing,
  fetchAccountProfile,
} from "@/infrastructure/api/account";
import { AccountRow } from "@/ui/components/AccountRow";
import { AppShell } from "@/ui/components/AppShell";
import { LoadMoreFooter } from "@/ui/components/LoadMoreFooter";
import { useSession } from "@/ui/context/SessionContext";
import { usePagePrefetch } from "@/ui/hooks/usePagePrefetch";
import { TIMELINE_PAGE_LIMIT, pageHasMore } from "@/ui/lib/pagination";

type CollectionKind = "followers" | "following";

export const AccountCollectionPage = ({ kind }: Readonly<{ kind: CollectionKind }>) => {
  const { accountId: routeAccountId } = useParams();
  const { session } = useSession();
  const selfId = session.kind === "Authenticated" ? session.account.id : null;
  const accountId = routeAccountId ?? selfId;
  const [profile, setProfile] = useState<AccountProfile | null>(null);
  const [accounts, setAccounts] = useState<ReadonlyArray<AccountProfile>>([]);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const [error, setError] = useState("");
  const loadingMoreRef = useRef(false);
  const accountsRef = useRef(accounts);
  accountsRef.current = accounts;

  const title = kind === "followers" ? "フォロワー" : "フォロー中";
  const fetchPage = kind === "followers" ? fetchAccountFollowers : fetchAccountFollowing;
  const prefetch = usePagePrefetch(async (maxId: string) => {
    if (!accountId) {
      return [];
    }
    const result = await fetchPage(accountId, { maxId, limit: TIMELINE_PAGE_LIMIT });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    return result.value;
  });

  const load = useCallback(
    async (id: string) => {
      const result = await fetchPage(id, { limit: TIMELINE_PAGE_LIMIT });
      if (result.isErr()) {
        throw new Error(mastodonErrorMessage(result.error));
      }
      setHasMore(pageHasMore(result.value.length));
      setAccounts(result.value);
      prefetch.prepareNext(result.value, result.value.length);
    },
    [fetchPage, prefetch],
  );

  useEffect(() => {
    if (!accountId) {
      return;
    }
    let active = true;
    setLoading(true);
    setError("");
    void Promise.all([fetchAccountProfile(accountId), load(accountId)])
      .then(([profileResult]) => {
        if (!active) {
          return;
        }
        if (profileResult.isErr()) {
          throw new Error(mastodonErrorMessage(profileResult.error));
        }
        setProfile(profileResult.value);
      })
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
  }, [accountId, load]);

  const handleLoadMore = async () => {
    if (!accountId) {
      return;
    }
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
        <Link to={accountId ? `/profile/${accountId}` : "/profile"}>← プロフィールに戻る</Link>
      </p>
      {profile ? (
        <p className="app-muted">
          @{profile.acct} の{title}
        </p>
      ) : null}
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

export const AccountFollowersPage = () => <AccountCollectionPage kind="followers" />;

export const AccountFollowingPage = () => <AccountCollectionPage kind="following" />;
