import { useEffect, useState, type FormEvent } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { SearchResults } from "@/domain/search/search";
import { Status } from "@/domain/status/status";
import { search } from "@/infrastructure/api/search";
import { AccountRow } from "@/ui/components/AccountRow";
import { AppShell } from "@/ui/components/AppShell";
import { StatusCard } from "@/ui/components/StatusCard";
import { useSession } from "@/ui/context/SessionContext";
import { useStatusActions } from "@/ui/hooks/useStatusActions";

const emptyResults = (): SearchResults => ({
  accounts: [],
  statuses: [],
  hashtags: [],
});

export const SearchPage = () => {
  const { session } = useSession();
  const selfAccountId = session.kind === "Authenticated" ? session.account.id : null;
  const [searchParams, setSearchParams] = useSearchParams();
  const queryFromUrl = searchParams.get("q") ?? "";
  const [query, setQuery] = useState(queryFromUrl);
  const [results, setResults] = useState<SearchResults>(emptyResults);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [hasSearched, setHasSearched] = useState(Boolean(queryFromUrl.trim()));

  const runSearch = async (nextQuery: string) => {
    const trimmed = nextQuery.trim();
    if (!trimmed) {
      setResults(emptyResults());
      setHasSearched(false);
      return;
    }
    setLoading(true);
    setError("");
    setHasSearched(true);
    const result = await search({ q: trimmed });
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      setLoading(false);
      return;
    }
    setResults(result.value);
    setLoading(false);
  };

  useEffect(() => {
    setQuery(queryFromUrl);
    if (queryFromUrl.trim()) {
      void runSearch(queryFromUrl);
    }
  }, [queryFromUrl]);

  const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmed = query.trim();
    if (!trimmed) {
      setSearchParams({});
      return;
    }
    setSearchParams({ q: trimmed });
  };

  const actions = useStatusActions({
    selfAccountId,
    onReplace: (updated) =>
      setResults((current) => ({
        ...current,
        statuses: Status.replaceInList(current.statuses, updated),
      })),
    onRemove: (statusId) =>
      setResults((current) => ({
        ...current,
        statuses: Status.removeById(current.statuses, statusId),
      })),
    onError: setError,
  });

  const hasResults =
    results.accounts.length > 0 || results.statuses.length > 0 || results.hashtags.length > 0;

  return (
    <AppShell title="検索">
      <form className="search-form app-card" onSubmit={handleSubmit}>
        <label className="search-label" htmlFor="search-query">
          キーワード
        </label>
        <div className="search-controls">
          <input
            id="search-query"
            className="search-input"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="アカウント、投稿、ハッシュタグ"
            autoComplete="off"
          />
          <button type="submit" className="app-button" disabled={loading}>
            {loading ? "検索中…" : "検索"}
          </button>
        </div>
      </form>
      {error ? <p className="app-error">{error}</p> : null}
      {loading ? <div className="app-status">読み込み中…</div> : null}
      {!loading && hasSearched && !hasResults ? (
        <div className="app-card">
          <p className="app-muted">「{queryFromUrl || query}」に一致する結果はありませんでした。</p>
        </div>
      ) : null}
      {!loading && results.accounts.length > 0 ? (
        <section className="search-section">
          <h2>アカウント</h2>
          <div className="search-accounts">
            {results.accounts.map((account) => (
              <AccountRow key={account.id} account={account} />
            ))}
          </div>
        </section>
      ) : null}
      {!loading && results.statuses.length > 0 ? (
        <section className="search-section">
          <h2>投稿</h2>
          <div className="timeline">
            {results.statuses.map((status) => (
              <StatusCard key={status.id} status={status} {...actions} />
            ))}
          </div>
        </section>
      ) : null}
      {!loading && results.hashtags.length > 0 ? (
        <section className="search-section">
          <h2>ハッシュタグ</h2>
          <ul className="search-hashtags">
            {results.hashtags.map((tag) => (
              <li key={tag.id}>
                <Link className="search-hashtag" to={`/tags/${encodeURIComponent(tag.name)}`}>
                  #{tag.name}
                </Link>
              </li>
            ))}
          </ul>
        </section>
      ) : null}
    </AppShell>
  );
};
