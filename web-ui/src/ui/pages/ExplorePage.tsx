import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { AccountSuggestion } from "@/domain/suggestions/suggestion";
import { Status } from "@/domain/status/status";
import type { TrendLink } from "@/domain/trends/link";
import { fetchSuggestions } from "@/infrastructure/api/suggestions";
import { fetchTrendingLinks, fetchTrendingStatuses } from "@/infrastructure/api/trends";
import { AccountRow } from "@/ui/components/AccountRow";
import { AppShell } from "@/ui/components/AppShell";
import { SearchSidebar } from "@/ui/components/SearchSidebar";
import { StatusCard } from "@/ui/components/StatusCard";
import { TrendsSidebar } from "@/ui/components/TrendsSidebar";
import { useSession } from "@/ui/context/SessionContext";
import { useStatusActions } from "@/ui/hooks/useStatusActions";

export const ExplorePage = () => {
  const { session } = useSession();
  const selfAccountId = session.kind === "Authenticated" ? session.account.id : null;
  const [statuses, setStatuses] = useState<ReadonlyArray<Status>>([]);
  const [links, setLinks] = useState<ReadonlyArray<TrendLink>>([]);
  const [suggestions, setSuggestions] = useState<ReadonlyArray<AccountSuggestion>>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  useEffect(() => {
    let active = true;
    setLoading(true);
    void Promise.all([fetchTrendingStatuses(), fetchTrendingLinks(), fetchSuggestions()]).then(
      ([statusResult, linkResult, suggestionResult]) => {
        if (!active) {
          return;
        }
        if (statusResult.isErr()) {
          setError(mastodonErrorMessage(statusResult.error));
        } else {
          setStatuses(statusResult.value);
        }
        if (linkResult.isOk()) {
          setLinks(linkResult.value);
        }
        if (suggestionResult.isOk()) {
          setSuggestions(suggestionResult.value);
        }
        setLoading(false);
      },
    );
    return () => {
      active = false;
    };
  }, []);

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
              <AccountRow key={suggestion.account.id} account={suggestion.account} />
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
        </section>
      ) : null}
      <section className="explore-section">
        <h2>トレンドの投稿</h2>
        <div className="timeline">
          {statuses.map((status) => (
            <StatusCard key={status.id} status={status} {...actions} />
          ))}
        </div>
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
