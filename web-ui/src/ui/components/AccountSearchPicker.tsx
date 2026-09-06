import { useState, type FormEvent } from "react";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { AccountProfile } from "@/domain/account/account";
import { search } from "@/infrastructure/api/search";

type AccountSearchPickerProps = Readonly<{
  placeholder?: string;
  excludeIds: ReadonlySet<string>;
  disabled?: boolean;
  onSelect: (account: AccountProfile) => void;
}>;

export const AccountSearchPicker = ({
  placeholder = "アカウントを検索",
  excludeIds,
  disabled = false,
  onSelect,
}: AccountSearchPickerProps) => {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<ReadonlyArray<AccountProfile>>([]);
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState("");

  const handleSearch = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmed = query.trim();
    if (!trimmed) {
      setResults([]);
      return;
    }
    setSearching(true);
    setError("");
    const result = await search({
      q: trimmed,
      type: "accounts",
      limit: 10,
      resolve: trimmed.includes("@"),
    });
    setSearching(false);
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    setResults(result.value.accounts.filter((account) => !excludeIds.has(account.id)));
  };

  return (
    <div className="account-search-picker">
      <form className="dm-search" onSubmit={(event) => void handleSearch(event)}>
        <input
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder={placeholder}
          disabled={disabled || searching}
        />
        <button type="submit" className="app-button app-button-secondary" disabled={disabled || searching}>
          {searching ? "検索中…" : "検索"}
        </button>
      </form>
      {error ? <p className="app-error">{error}</p> : null}
      {results.length > 0 ? (
        <div className="dm-search-results">
          {results.map((account) => (
            <button
              key={account.id}
              type="button"
              className="account-row dm-search-result"
              disabled={disabled || excludeIds.has(account.id)}
              onClick={() => {
                onSelect(account);
                setResults((current) => current.filter((item) => item.id !== account.id));
              }}
            >
              <img className="status-avatar" src={account.avatar} alt="" loading="lazy" />
              <div className="account-row-meta">
                <span className="status-display-name">{account.displayName || account.username}</span>
                <span className="status-acct">@{account.acct}</span>
              </div>
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
};
