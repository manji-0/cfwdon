import { useCallback, useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { AccountProfile } from "@/domain/account/account";
import {
  fetchAccountFollowers,
  fetchAccountFollowing,
  fetchAccountProfile,
} from "@/infrastructure/api/account";
import { AccountRow } from "@/ui/components/AccountRow";
import { AppShell } from "@/ui/components/AppShell";
import { useSession } from "@/ui/context/SessionContext";

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
  const [error, setError] = useState("");

  const title = kind === "followers" ? "フォロワー" : "フォロー中";
  const fetchPage = kind === "followers" ? fetchAccountFollowers : fetchAccountFollowing;

  const load = useCallback(
    async (id: string, options?: { maxId?: string; replace?: boolean }) => {
      const result = await fetchPage(id, { maxId: options?.maxId, limit: 20 });
      if (result.isErr()) {
        throw new Error(mastodonErrorMessage(result.error));
      }
      setAccounts((current) =>
        options?.replace || !options?.maxId ? result.value : [...current, ...result.value],
      );
    },
    [fetchPage],
  );

  useEffect(() => {
    if (!accountId) {
      return;
    }
    let active = true;
    setLoading(true);
    setError("");
    void Promise.all([fetchAccountProfile(accountId), load(accountId, { replace: true })])
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
    const last = accounts.at(-1);
    if (!last || loadingMore) {
      return;
    }
    setLoadingMore(true);
    try {
      await load(accountId, { maxId: last.id });
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : "続きの読み込みに失敗しました");
    } finally {
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
      {accounts.length > 0 ? (
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

export const AccountFollowersPage = () => <AccountCollectionPage kind="followers" />;

export const AccountFollowingPage = () => <AccountCollectionPage kind="following" />;
