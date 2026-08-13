import { useMemo, useState, type FormEvent } from "react";
import { Link, useNavigate } from "react-router-dom";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { AccountProfile } from "@/domain/account/account";
import { ensureDirectMentions } from "@/domain/conversations/mentions";
import { Visibility, type Visibility as StatusVisibility } from "@/domain/status/visibility";
import { findConversationByStatusId } from "@/infrastructure/api/conversations";
import { search } from "@/infrastructure/api/search";
import { createStatus } from "@/infrastructure/api/status";
import { AppShell } from "@/ui/components/AppShell";
import { Composer } from "@/ui/components/Composer";
import { useSession } from "@/ui/context/SessionContext";

export const NewMessagePage = () => {
  const navigate = useNavigate();
  const { session } = useSession();
  const selfId = session.kind === "Authenticated" ? session.account.id : "";
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<ReadonlyArray<AccountProfile>>([]);
  const [selected, setSelected] = useState<ReadonlyArray<AccountProfile>>([]);
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState("");

  const selectedIds = useMemo(() => new Set(selected.map((account) => account.id)), [selected]);

  const handleSearch = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmed = query.trim();
    if (!trimmed) {
      setResults([]);
      return;
    }
    setSearching(true);
    setError("");
    const result = await search({ q: trimmed, type: "accounts", limit: 10 });
    setSearching(false);
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    setResults(result.value.accounts.filter((account) => account.id !== selfId));
  };

  const addAccount = (account: AccountProfile) => {
    if (selectedIds.has(account.id)) {
      return;
    }
    setSelected((current) => [...current, account]);
  };

  const removeAccount = (accountId: string) => {
    setSelected((current) => current.filter((account) => account.id !== accountId));
  };

  const handleSend = async (input: {
    text: string;
    visibility: StatusVisibility;
    spoilerText: string;
    sensitive: boolean;
    inReplyToId?: string;
    mediaIds: ReadonlyArray<string>;
  }) => {
    if (selected.length === 0) {
      throw new Error("送信先を選んでください");
    }
    const result = await createStatus({
      text: ensureDirectMentions(input.text, selected),
      visibility: Visibility.toApi(Visibility.direct()),
      spoilerText: input.spoilerText,
      sensitive: input.sensitive,
      mediaIds: input.mediaIds,
    });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    const conversation = await findConversationByStatusId(result.value.id);
    if (conversation.isOk()) {
      navigate(`/messages/${conversation.value.id}`);
      return;
    }
    navigate("/messages");
  };

  return (
    <AppShell title="新しいメッセージ">
      <p className="thread-back">
        <Link to="/messages">← メッセージに戻る</Link>
      </p>
      {error ? <p className="app-error">{error}</p> : null}
      <form className="dm-search" onSubmit={(event) => void handleSearch(event)}>
        <input
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="アカウントを検索"
        />
        <button type="submit" className="app-button app-button-secondary" disabled={searching}>
          {searching ? "検索中…" : "検索"}
        </button>
      </form>
      {selected.length > 0 ? (
        <div className="dm-chips">
          {selected.map((account) => (
            <button
              key={account.id}
              type="button"
              className="dm-chip"
              onClick={() => removeAccount(account.id)}
            >
              {account.displayName || account.username} ×
            </button>
          ))}
        </div>
      ) : (
        <p className="app-muted">1人以上の送信先を選んでください。</p>
      )}
      {results.length > 0 ? (
        <div className="dm-search-results">
          {results.map((account) => (
            <button
              key={account.id}
              type="button"
              className="account-row dm-search-result"
              disabled={selectedIds.has(account.id)}
              onClick={() => addAccount(account)}
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
      <Composer
        placeholder="メッセージを書く"
        submitLabel="送信"
        initialVisibility={Visibility.direct()}
        lockVisibility
        disabled={selected.length === 0}
        onSubmit={handleSend}
      />
    </AppShell>
  );
};
