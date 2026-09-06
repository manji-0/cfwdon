import { useCallback, useEffect, useRef, useState } from "react";
import { Link } from "react-router";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { AccountSuggestion } from "@/domain/suggestions/suggestion";
import { Status } from "@/domain/status/status";
import type { TrendLink } from "@/domain/trends/link";
import { dismissSuggestion, fetchSuggestions } from "@/infrastructure/api/suggestions";
import { fetchTrendingLinks, fetchTrendingStatuses } from "@/infrastructure/api/trends";
import { AccountRow } from "@/ui/components/AccountRow";
import { AppShell } from "@/ui/components/AppShell";
import { LoadMoreFooter } from "@/ui/components/LoadMoreFooter";
import { SearchSidebar } from "@/ui/components/SearchSidebar";
import { StatusCard } from "@/ui/components/StatusCard";
import { TrendsSidebar } from "@/ui/components/TrendsSidebar";
import { useSession } from "@/ui/context/SessionContext";
import { useStatusActions } from "@/ui/hooks/useStatusActions";
import { TIMELINE_PAGE_LIMIT, pageHasMore } from "@/ui/lib/pagination";

const appendUniqueLinks = (
  current: ReadonlyArray<TrendLink>,
  incoming: ReadonlyArray<TrendLink>,
): ReadonlyArray<TrendLink> => {
  const seen = new Set(current.map((link) => link.url));
  const next = incoming.filter((link) => !seen.has(link.url));
  return next.length === 0 ? current : [...current, ...next];
};

export const ExplorePage = () => {
  const { session } = useSession();
  const selfAccountId = session.kind === "Authenticated" ? session.account.id : null;
  const [statuses, setStatuses] = useState<ReadonlyArray<Status>>([]);
  const [links, setLinks] = useState<ReadonlyArray<TrendLink>>([]);
  const [suggestions, setSuggestions] = useState<ReadonlyArray<AccountSuggestion>>([]);
  const [loading, setLoading] = useState(true);
  const [loadingMoreStatuses, setLoadingMoreStatuses] = useState(false);
  const [loadingMoreLinks, setLoadingMoreLinks] = useState(false);
  const [statusesHasMore, setStatusesHasMore] = useState(false);
  const [linksHasMore, setLinksHasMore] = useState(false);
  const [dismissingId, setDismissingId] = useState<string | null>(null);
  const [error, setError] = useState("");
  const loadingMoreStatusesRef = useRef(false);
  const loadingMoreLinksRef = useRef(false);

  useEffect(() => {
    let active = true;
    setLoading(true);
    void Promise.all([
      fetchTrendingStatuses({ limit: TIMELINE_PAGE_LIMIT }),
      fetchTrendingLinks({ limit: TIMELINE_PAGE_LIMIT }),
      fetchSuggestions(),
    ]).then(([statusResult, linkResult, suggestionResult]) => {
      if (!active) {
        return;
      }
      if (statusResult.isErr()) {
        setError(mastodonErrorMessage(statusResult.error));
      } else {
        setStatuses(statusResult.value);
        setStatusesHasMore(pageHasMore(statusResult.value.length));
      }
      if (linkResult.isOk()) {
        setLinks(linkResult.value);
        setLinksHasMore(pageHasMore(linkResult.value.length));
      }
      if (suggestionResult.isOk()) {
        setSuggestions(suggestionResult.value);
      }
      setLoading(false);
    });
    return () => {
      active = false;
    };
  }, []);

  const handleLoadMoreStatuses = useCallback(async () => {
    if (loadingMoreStatusesRef.current) {
      return;
    }
    loadingMoreStatusesRef.current = true;
    setLoadingMoreStatuses(true);
    const result = await fetchTrendingStatuses({
      limit: TIMELINE_PAGE_LIMIT,
      offset: statuses.length,
    });
    loadingMoreStatusesRef.current = false;
    setLoadingMoreStatuses(false);
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    setStatusesHasMore(pageHasMore(result.value.length));
    setStatuses((current) => Status.appendUnique(current, result.value));
  }, [statuses.length]);

  const handleLoadMoreLinks = useCallback(async () => {
    if (loadingMoreLinksRef.current) {
      return;
    }
    loadingMoreLinksRef.current = true;
    setLoadingMoreLinks(true);
    const result = await fetchTrendingLinks({
      limit: TIMELINE_PAGE_LIMIT,
      offset: links.length,
    });
    loadingMoreLinksRef.current = false;
    setLoadingMoreLinks(false);
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    setLinksHasMore(pageHasMore(result.value.length));
    setLinks((current) => appendUniqueLinks(current, result.value));
  }, [links.length]);

  const handleDismissSuggestion = async (accountId: string) => {
    setDismissingId(accountId);
    const result = await dismissSuggestion(accountId);
    setDismissingId(null);
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    setSuggestions((current) => current.filter((item) => item.account.id !== accountId));
  };

  const actions = useStatusActions({
    selfAccountId,
    onReplace: (updated) => setStatuses((current) => Status.replaceInList(current, updated)),
    onRemove: (statusId) => setStatuses((current) => Status.removeById(current, statusId)),
    onError: setError,
  });

  return (
    <AppShell
      title="探索"
      aside={
        <>
          <SearchSidebar />
          <TrendsSidebar />
        </>
      }
    >
      {error ? <p className="app-error">{error}</p> : null}
      {loading ? <div className="app-status">読み込み中…</div> : null}
      {suggestions.length > 0 ? (
        <section className="app-card explore-section">
          <h2>おすすめアカウント</h2>
          <div className="explore-accounts">
            {suggestions.map((suggestion) => (
              <AccountRow
                key={suggestion.account.id}
                account={suggestion.account}
                actionLabel="非表示"
                actionDisabled={dismissingId === suggestion.account.id}
                onAction={() => void handleDismissSuggestion(suggestion.account.id)}
              />
            ))}
          </div>
        </section>
      ) : null}
      {links.length > 0 ? (
        <section className="app-card explore-section">
          <h2>トレンドリンク</h2>
          <ul className="explore-links">
            {links.map((link) => (
              <li key={link.url}>
                <a href={link.url} target="_blank" rel="noreferrer">
                  {link.image ? <img src={link.image} alt="" /> : null}
                  <span>
                    <strong>{link.title}</strong>
                    {link.description ? <span className="app-muted">{link.description}</span> : null}
                  </span>
                </a>
              </li>
            ))}
          </ul>
          <LoadMoreFooter
            hasMore={linksHasMore}
            loading={loadingMoreLinks}
            observeKey={links.length}
            onLoadMore={() => void handleLoadMoreLinks()}
          />
        </section>
      ) : null}
      <section className="explore-section">
        <h2>トレンドの投稿</h2>
        <div className="timeline">
          {statuses.map((status) => (
            <StatusCard key={status.id} status={status} {...actions} />
          ))}
        </div>
        <LoadMoreFooter
          hasMore={statusesHasMore}
          loading={loadingMoreStatuses}
          observeKey={statuses.length}
          onLoadMore={() => void handleLoadMoreStatuses()}
        />
        {!loading && statuses.length === 0 ? (
          <div className="app-card">
            <p className="app-muted">トレンドの投稿はまだありません。</p>
            <Link to="/search">検索へ</Link>
          </div>
        ) : null}
      </section>
    </AppShell>
  );
};
