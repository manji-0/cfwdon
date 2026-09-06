import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { Link, useSearchParams } from "react-router";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import {
  SearchType,
  emptySearchResults,
  type SearchResults,
  type SearchType as SearchTypeValue,
} from "@/domain/search/search";
import { Status } from "@/domain/status/status";
import { search } from "@/infrastructure/api/search";
import { AccountRow } from "@/ui/components/AccountRow";
import { AppShell } from "@/ui/components/AppShell";
import { LoadMoreFooter } from "@/ui/components/LoadMoreFooter";
import { StatusCard } from "@/ui/components/StatusCard";
import { useSession } from "@/ui/context/SessionContext";
import { useStatusActions } from "@/ui/hooks/useStatusActions";
import { TIMELINE_PAGE_LIMIT, pageHasMore } from "@/ui/lib/pagination";

const typedCount = (results: SearchResults, type: SearchTypeValue): number => {
  switch (type) {
    case "accounts":
      return results.accounts.length;
    case "statuses":
      return results.statuses.length;
    case "hashtags":
      return results.hashtags.length;
    default:
      return results.accounts.length + results.statuses.length + results.hashtags.length;
  }
};

const mergeResults = (
  current: SearchResults,
  page: SearchResults,
  type: SearchTypeValue,
): SearchResults => {
  switch (type) {
    case "accounts":
      return { ...current, accounts: [...current.accounts, ...page.accounts] };
    case "statuses":
      return { ...current, statuses: [...current.statuses, ...page.statuses] };
    case "hashtags":
      return { ...current, hashtags: [...current.hashtags, ...page.hashtags] };
    default:
      return page;
  }
};

export const SearchPage = () => {
  const { session } = useSession();
  const selfAccountId = session.kind === "Authenticated" ? session.account.id : null;
  const [searchParams, setSearchParams] = useSearchParams();
  const queryFromUrl = searchParams.get("q") ?? "";
  const typeFromUrl = SearchType.fromParam(searchParams.get("type"));
  const [query, setQuery] = useState(queryFromUrl);
  const [results, setResults] = useState<SearchResults>(emptySearchResults());
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [hasMore, setHasMore] = useState(false);
  const [error, setError] = useState("");
  const [hasSearched, setHasSearched] = useState(Boolean(queryFromUrl.trim()));
  const loadingMoreRef = useRef(false);
  const resultsRef = useRef(results);
  resultsRef.current = results;

  const runSearch = useCallback(
    async (nextQuery: string, type: SearchTypeValue, offset = 0) => {
      const trimmed = nextQuery.trim();
      if (!trimmed) {
        setResults(emptySearchResults());
        setHasSearched(false);
        setHasMore(false);
        return emptySearchResults();
      }
      const result = await search({
        q: trimmed,
        type: type === "all" ? undefined : type,
        limit: TIMELINE_PAGE_LIMIT,
        offset,
        resolve: SearchType.shouldResolve(trimmed),
      });
      if (result.isErr()) {
        throw new Error(mastodonErrorMessage(result.error));
      }
      return result.value;
    },
    [],
  );

  useEffect(() => {
    setQuery(queryFromUrl);
    const trimmed = queryFromUrl.trim();
    if (!trimmed) {
      setResults(emptySearchResults());
      setHasSearched(false);
      setHasMore(false);
      setLoading(false);
      return;
    }
    let active = true;
    setLoading(true);
    setError("");
    setHasSearched(true);
    void runSearch(trimmed, typeFromUrl)
      .then((page) => {
        if (!active) {
          return;
        }
        setResults(page);
        setHasMore(typeFromUrl !== "all" && pageHasMore(typedCount(page, typeFromUrl)));
      })
      .catch((loadError: unknown) => {
        if (active) {
          setError(loadError instanceof Error ? loadError.message : "検索に失敗しました");
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
  }, [queryFromUrl, typeFromUrl, runSearch]);

  const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmed = query.trim();
    if (!trimmed) {
      setSearchParams({});
      return;
    }
    const next = new URLSearchParams();
    next.set("q", trimmed);
    if (typeFromUrl !== "all") {
      next.set("type", typeFromUrl);
    }
    setSearchParams(next);
  };

  const handleTypeChange = (type: SearchTypeValue) => {
    const trimmed = (queryFromUrl || query).trim();
    const next = new URLSearchParams();
    if (trimmed) {
      next.set("q", trimmed);
    }
    if (type !== "all") {
      next.set("type", type);
    }
    setSearchParams(next);
  };

  const handleLoadMore = async () => {
    if (typeFromUrl === "all" || loadingMoreRef.current) {
      return;
    }
    loadingMoreRef.current = true;
    setLoadingMore(true);
    setError("");
    try {
      const page = await runSearch(
        queryFromUrl,
        typeFromUrl,
        typedCount(resultsRef.current, typeFromUrl),
      );
      if (pageHasMore(typedCount(page, typeFromUrl))) {
        setHasMore(true);
      } else {
        setHasMore(false);
      }
      setResults((current) => mergeResults(current, page, typeFromUrl));
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : "続きの読み込みに失敗しました");
    } finally {
      loadingMoreRef.current = false;
      setLoadingMore(false);
    }
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

  const hasResults = typedCount(results, "all") > 0;
  const showAccounts = typeFromUrl === "all" || typeFromUrl === "accounts";
  const showStatuses = typeFromUrl === "all" || typeFromUrl === "statuses";
  const showHashtags = typeFromUrl === "all" || typeFromUrl === "hashtags";

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
            placeholder="アカウント、投稿、ハッシュタグ、URL"
            autoComplete="off"
          />
          <button type="submit" className="app-button" disabled={loading}>
            {loading ? "検索中…" : "検索"}
          </button>
        </div>
      </form>
      <nav className="timeline-tabs" aria-label="検索の種類">
        {SearchType.values.map((type) => (
          <button
            key={type}
            type="button"
            className={typeFromUrl === type ? "is-active" : undefined}
            onClick={() => handleTypeChange(type)}
          >
            {SearchType.label(type)}
          </button>
        ))}
      </nav>
      {error ? <p className="app-error">{error}</p> : null}
      {loading ? <div className="app-status">読み込み中…</div> : null}
      {!loading && hasSearched && !hasResults ? (
        <div className="app-card">
          <p className="app-muted">「{queryFromUrl || query}」に一致する結果はありませんでした。</p>
        </div>
      ) : null}
      {!loading && showAccounts && results.accounts.length > 0 ? (
        <section className="search-section">
          <h2>アカウント</h2>
          <div className="search-accounts">
            {results.accounts.map((account) => (
              <AccountRow key={account.id} account={account} />
            ))}
          </div>
        </section>
      ) : null}
      {!loading && showStatuses && results.statuses.length > 0 ? (
        <section className="search-section">
          <h2>投稿</h2>
          <div className="timeline">
            {results.statuses.map((status) => (
              <StatusCard key={status.id} status={status} {...actions} />
            ))}
          </div>
        </section>
      ) : null}
      {!loading && showHashtags && results.hashtags.length > 0 ? (
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
      <LoadMoreFooter
        hasMore={hasMore && !loading && hasSearched}
        loading={loadingMore}
        observeKey={typedCount(results, typeFromUrl)}
        onLoadMore={() => void handleLoadMore()}
      />
    </AppShell>
  );
};
