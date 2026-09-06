import { useCallback, useEffect, useRef, useState } from "react";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { AccountRef } from "@/domain/account/account";
import type { AccountList } from "@/domain/lists/list";
import {
  ListRepliesPolicy,
  type ListRepliesPolicy as ListRepliesPolicyValue,
} from "@/domain/lists/replies-policy";
import { Status } from "@/domain/status/status";
import {
  addListAccounts,
  createList,
  deleteList,
  fetchListAccounts,
  fetchListTimeline,
  fetchLists,
  removeListAccounts,
  updateList,
} from "@/infrastructure/api/lists";
import { WebUiPhase } from "@/plan/phases";
import { AccountSearchPicker } from "@/ui/components/AccountSearchPicker";
import { AppShell } from "@/ui/components/AppShell";
import { LoadMoreFooter } from "@/ui/components/LoadMoreFooter";
import { StatusCard } from "@/ui/components/StatusCard";
import { useConfirm } from "@/ui/context/ConfirmContext";
import { useSession } from "@/ui/context/SessionContext";
import { useStatusActions } from "@/ui/hooks/useStatusActions";
import { usePagePrefetch } from "@/ui/hooks/usePagePrefetch";
import { TIMELINE_PAGE_LIMIT, pageHasMore } from "@/ui/lib/pagination";

export const ListsPage = () => {
  const { session } = useSession();
  const { confirm } = useConfirm();
  const selfAccountId = session.kind === "Authenticated" ? session.account.id : null;
  const [lists, setLists] = useState<ReadonlyArray<AccountList>>([]);
  const [selectedListId, setSelectedListId] = useState<string | null>(null);
  const [statuses, setStatuses] = useState<ReadonlyArray<Status>>([]);
  const [members, setMembers] = useState<ReadonlyArray<AccountRef>>([]);
  const [loadingLists, setLoadingLists] = useState(true);
  const [loadingTimeline, setLoadingTimeline] = useState(false);
  const [loadingMembers, setLoadingMembers] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const loadingMoreRef = useRef(false);
  const statusesRef = useRef(statuses);
  statusesRef.current = statuses;
  const prefetch = usePagePrefetch(async (maxId: string) => {
    if (!selectedListId) {
      return [];
    }
    const result = await fetchListTimeline(selectedListId, { maxId, limit: TIMELINE_PAGE_LIMIT });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    return result.value;
  });

  const [createTitle, setCreateTitle] = useState("");
  const [createPolicy, setCreatePolicy] = useState<ListRepliesPolicyValue>(
    ListRepliesPolicy.defaultValue(),
  );
  const [createExclusive, setCreateExclusive] = useState(false);

  const [editTitle, setEditTitle] = useState("");
  const [editPolicy, setEditPolicy] = useState<ListRepliesPolicyValue>(
    ListRepliesPolicy.defaultValue(),
  );
  const [editExclusive, setEditExclusive] = useState(false);

  const selectedList = lists.find((list) => list.id === selectedListId) ?? null;

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
        setSelectedListId((current) => current ?? result.value[0]?.id ?? null);
      }
      setLoadingLists(false);
    })();
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    if (!selectedList) {
      setEditTitle("");
      setEditPolicy(ListRepliesPolicy.defaultValue());
      setEditExclusive(false);
      return;
    }
    setEditTitle(selectedList.title);
    setEditPolicy(ListRepliesPolicy.fromApi(selectedList.repliesPolicy));
    setEditExclusive(selectedList.exclusive);
  }, [selectedList]);

  const loadTimeline = useCallback(
    async (listId: string) => {
      const result = await fetchListTimeline(listId, { limit: TIMELINE_PAGE_LIMIT });
      if (result.isErr()) {
        throw new Error(mastodonErrorMessage(result.error));
      }
      setHasMore(pageHasMore(result.value.length));
      setStatuses(result.value);
      prefetch.prepareNext(result.value, result.value.length);
    },
    [prefetch],
  );

  const loadMembers = useCallback(async (listId: string) => {
    const result = await fetchListAccounts(listId);
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    setMembers(result.value);
  }, []);

  useEffect(() => {
    if (!selectedListId) {
      setStatuses([]);
      setMembers([]);
      prefetch.reset();
      return;
    }
    let active = true;
    setLoadingTimeline(true);
    setLoadingMembers(true);
    setError("");
    void Promise.all([
      loadTimeline(selectedListId),
      loadMembers(selectedListId),
    ])
      .catch((loadError) => {
        if (active) {
          setError(loadError instanceof Error ? loadError.message : "リストの読み込みに失敗しました");
        }
      })
      .finally(() => {
        if (active) {
          setLoadingTimeline(false);
          setLoadingMembers(false);
        }
      });
    return () => {
      active = false;
    };
  }, [selectedListId, loadTimeline, loadMembers, prefetch]);

  const actions = useStatusActions({
    selfAccountId,
    onReplace: (updated) => setStatuses((current) => Status.replaceInList(current, updated)),
    onRemove: (statusId) => setStatuses((current) => Status.removeById(current, statusId)),
    onError: setError,
  });

  const handleLoadMore = async () => {
    if (!selectedListId) {
      return;
    }
    const last = statusesRef.current.at(-1);
    if (!last || loadingMoreRef.current) {
      return;
    }
    loadingMoreRef.current = true;
    if (!prefetch.isReady()) {
      setLoadingMore(true);
    }
    setError("");
    try {
      const page = await prefetch.takeNext(last.id);
      if (page.length === 0) {
        setHasMore(false);
        return;
      }
      const next = [...statusesRef.current, ...page];
      setHasMore(pageHasMore(page.length));
      setStatuses(next);
      prefetch.prepareNext(next, page.length);
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : "続きの読み込みに失敗しました");
    } finally {
      loadingMoreRef.current = false;
      setLoadingMore(false);
    }
  };

  const handleCreateList = async () => {
    const title = createTitle.trim();
    if (!title || saving) {
      return;
    }
    setSaving(true);
    setError("");
    const result = await createList({
      title,
      repliesPolicy: createPolicy,
      exclusive: createExclusive,
    });
    setSaving(false);
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    setLists((current) => [...current, result.value]);
    setSelectedListId(result.value.id);
    setCreateTitle("");
    setCreatePolicy(ListRepliesPolicy.defaultValue());
    setCreateExclusive(false);
  };

  const handleUpdateList = async () => {
    if (!selectedListId) {
      return;
    }
    const title = editTitle.trim();
    if (!title || saving) {
      return;
    }
    setSaving(true);
    setError("");
    const result = await updateList(selectedListId, {
      title,
      repliesPolicy: editPolicy,
      exclusive: editExclusive,
    });
    setSaving(false);
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    setLists((current) =>
      current.map((list) => (list.id === result.value.id ? result.value : list)),
    );
  };

  const handleDeleteList = async () => {
    if (!selectedListId || saving) {
      return;
    }
    const ok = await confirm("このリストを削除しますか？", {
      title: "リストの削除",
      confirmLabel: "削除",
      danger: true,
    });
    if (!ok) {
      return;
    }
    setSaving(true);
    setError("");
    const listId = selectedListId;
    const result = await deleteList(listId);
    setSaving(false);
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    setLists((current) => {
      const next = current.filter((list) => list.id !== listId);
      setSelectedListId(next[0]?.id ?? null);
      return next;
    });
  };

  const handleAddMember = async (accountId: string) => {
    if (!selectedListId) {
      return;
    }
    if (!accountId || saving) {
      return;
    }
    setSaving(true);
    setError("");
    const result = await addListAccounts(selectedListId, [accountId]);
    if (result.isErr()) {
      setSaving(false);
      setError(mastodonErrorMessage(result.error));
      return;
    }
    try {
      await loadMembers(selectedListId);
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : "メンバーの再読み込みに失敗しました");
    } finally {
      setSaving(false);
    }
  };

  const handleRemoveMember = async (accountId: string) => {
    if (!selectedListId || saving) {
      return;
    }
    setSaving(true);
    setError("");
    const result = await removeListAccounts(selectedListId, [accountId]);
    if (result.isErr()) {
      setSaving(false);
      setError(mastodonErrorMessage(result.error));
      return;
    }
    setMembers((current) => current.filter((member) => member.id !== accountId));
    setSaving(false);
  };

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
          <form
            className="list-form"
            onSubmit={(event) => {
              event.preventDefault();
              void handleCreateList();
            }}
          >
            <h3 className="list-form-title">新規作成</h3>
            <label className="list-form-field">
              <span className="app-muted">タイトル</span>
              <input
                value={createTitle}
                onChange={(event) => setCreateTitle(event.target.value)}
                placeholder="リスト名"
                required
                disabled={saving}
              />
            </label>
            <label className="list-form-field">
              <span className="app-muted">返信の表示</span>
              <select
                value={createPolicy}
                onChange={(event) =>
                  setCreatePolicy(ListRepliesPolicy.fromApi(event.target.value))
                }
                disabled={saving}
              >
                {ListRepliesPolicy.values.map((policy) => (
                  <option key={policy} value={policy}>
                    {ListRepliesPolicy.label(policy)}
                  </option>
                ))}
              </select>
            </label>
            <label className="list-form-check">
              <input
                type="checkbox"
                checked={createExclusive}
                onChange={(event) => setCreateExclusive(event.target.checked)}
                disabled={saving}
              />
              ホームから除外（exclusive）
            </label>
            <button type="submit" className="app-button" disabled={saving || !createTitle.trim()}>
              作成
            </button>
          </form>
        </div>
      }
    >
      <div data-phase={WebUiPhase.collections}>
        {error ? <p className="app-error">{error}</p> : null}
        {selectedList ? (
          <>
            <h2 className="library-section-title">{selectedList.title || "無題のリスト"}</h2>
            <section className="app-card list-manage-panel">
              <h3>リスト設定</h3>
              <form
                className="list-form list-form-inline"
                onSubmit={(event) => {
                  event.preventDefault();
                  void handleUpdateList();
                }}
              >
                <label className="list-form-field">
                  <span className="app-muted">タイトル</span>
                  <input
                    value={editTitle}
                    onChange={(event) => setEditTitle(event.target.value)}
                    required
                    disabled={saving}
                  />
                </label>
                <label className="list-form-field">
                  <span className="app-muted">返信の表示</span>
                  <select
                    value={editPolicy}
                    onChange={(event) =>
                      setEditPolicy(ListRepliesPolicy.fromApi(event.target.value))
                    }
                    disabled={saving}
                  >
                    {ListRepliesPolicy.values.map((policy) => (
                      <option key={policy} value={policy}>
                        {ListRepliesPolicy.label(policy)}
                      </option>
                    ))}
                  </select>
                </label>
                <label className="list-form-check">
                  <input
                    type="checkbox"
                    checked={editExclusive}
                    onChange={(event) => setEditExclusive(event.target.checked)}
                    disabled={saving}
                  />
                  ホームから除外（exclusive）
                </label>
                <div className="list-form-actions">
                  <button
                    type="submit"
                    className="app-button"
                    disabled={saving || !editTitle.trim()}
                  >
                    更新
                  </button>
                  <button
                    type="button"
                    className="app-button app-button-secondary"
                    onClick={() => void handleDeleteList()}
                    disabled={saving}
                  >
                    削除
                  </button>
                </div>
              </form>
            </section>

            <section className="app-card list-manage-panel">
              <h3>メンバー</h3>
              {loadingMembers ? <p className="app-muted">読み込み中…</p> : null}
              <ul className="list-member-list">
                {members.map((member) => (
                  <li key={member.id} className="list-member-row">
                    <img className="status-avatar" src={member.avatar} alt="" loading="lazy" />
                    <div className="list-member-meta">
                      <span className="status-display-name">
                        {member.displayName || member.username}
                      </span>
                      <span className="status-acct">@{member.acct}</span>
                    </div>
                    <button
                      type="button"
                      className="app-button app-button-secondary"
                      onClick={() => void handleRemoveMember(member.id)}
                      disabled={saving}
                    >
                      外す
                    </button>
                  </li>
                ))}
              </ul>
              {!loadingMembers && members.length === 0 ? (
                <p className="app-muted">メンバーはまだいません。</p>
              ) : null}
              <AccountSearchPicker
                placeholder="アカウント名で検索して追加"
                excludeIds={new Set(members.map((member) => member.id))}
                disabled={saving}
                onSelect={(account) => void handleAddMember(account.id)}
              />
            </section>
          </>
        ) : null}
        {loadingTimeline ? <div className="app-status">読み込み中…</div> : null}
        <div className="timeline">
          {statuses.map((status) => (
            <StatusCard key={status.id} status={status} {...actions} />
          ))}
        </div>
        {!loadingLists && !loadingTimeline && selectedList && statuses.length === 0 ? (
          <div className="app-card">
            <p className="app-muted">このリストにはまだ投稿がありません。</p>
          </div>
        ) : null}
        <LoadMoreFooter
          hasMore={hasMore && !loadingTimeline && statuses.length > 0}
          loading={loadingMore}
          observeKey={statuses.length}
          onLoadMore={() => void handleLoadMore()}
        />
      </div>
    </AppShell>
  );
};
