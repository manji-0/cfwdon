import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { AccountCredentials } from "@/domain/account/credentials";
import type { AccountRef } from "@/domain/account/account";
import { FilterContext, type KeywordFilter } from "@/domain/filters/filter";
import type { NotificationPolicy, NotificationPolicyAction } from "@/domain/settings/notification-policy";
import { NotificationPolicy as NotificationPolicyModel } from "@/domain/settings/notification-policy";
import { SessionState } from "@/domain/session/session";
import { Visibility } from "@/domain/status/visibility";
import type { FollowedTag } from "@/domain/tags/followed-tag";
import { AppRoute } from "@/domain/navigation/route";
import { fetchAccountCredentials, updateAccountProfile } from "@/infrastructure/api/credentials";
import { blockDomain, fetchDomainBlocks, unblockDomain } from "@/infrastructure/api/domain-blocks";
import {
  createKeywordFilter,
  deleteKeywordFilter,
  fetchKeywordFilters,
} from "@/infrastructure/api/filters";
import { fetchBlockedAccounts, fetchMutedAccounts } from "@/infrastructure/api/moderation";
import { unmuteAccount, unblockAccount } from "@/infrastructure/api/relationship";
import {
  fetchNotificationPolicy,
  updateNotificationPolicy,
} from "@/infrastructure/api/notification-policy";
import { fetchAccountPreferences, updatePostingPreferences } from "@/infrastructure/api/settings";
import { fetchFollowedTags, unfollowTag } from "@/infrastructure/api/tags";
import { WebUiPhase } from "@/plan/phases";
import { AppShell } from "@/ui/components/AppShell";
import { useSession } from "@/ui/context/SessionContext";
import { useUnreadMessages } from "@/ui/context/UnreadMessagesContext";
import { usePwaInstall } from "@/ui/hooks/usePwaInstall";

const POLICY_FIELDS = [
  { key: "forNotFollowing", label: "フォローしていないユーザー" },
  { key: "forNotFollowers", label: "フォロワーではないユーザー" },
  { key: "forNewAccounts", label: "新規アカウント" },
  { key: "forPrivateMentions", label: "非公開メンション" },
  { key: "forLimitedAccounts", label: "制限付きアカウント" },
] as const satisfies ReadonlyArray<{
  key: keyof NotificationPolicy;
  label: string;
}>;

const QUOTE_POLICY_OPTIONS = [
  { value: "public", label: "誰でも" },
  { value: "followers", label: "フォロワーのみ" },
  { value: "nobody", label: "許可しない" },
] as const;

const VISIBILITY_OPTIONS = [
  Visibility.public(),
  Visibility.unlisted(),
  Visibility.private(),
  Visibility.direct(),
] as const;

const ModerationAccountRow = ({
  account,
  actionLabel,
  onAction,
  disabled,
}: Readonly<{
  account: AccountRef;
  actionLabel: string;
  onAction: () => void;
  disabled: boolean;
}>) => (
  <div className="account-row settings-moderation-row">
    <Link className="settings-moderation-link" to={`/profile/${account.id}`}>
      <img className="status-avatar" src={account.avatar} alt="" loading="lazy" />
      <div className="account-row-meta">
        <span className="status-display-name">{account.displayName || account.username}</span>
        <span className="status-acct">@{account.acct}</span>
      </div>
    </Link>
    <button type="button" className="app-button app-button-secondary" onClick={onAction} disabled={disabled}>
      {actionLabel}
    </button>
  </div>
);

export const SettingsPage = () => {
  const { session, setSession, clearSession } = useSession();
  const { unreadCount } = useUnreadMessages();
  const { canInstall, installed, install } = usePwaInstall();

  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  const [credentials, setCredentials] = useState<AccountCredentials | null>(null);
  const [notificationPolicy, setNotificationPolicy] = useState<NotificationPolicy | null>(null);
  const [mutes, setMutes] = useState<ReadonlyArray<AccountRef>>([]);
  const [blocks, setBlocks] = useState<ReadonlyArray<AccountRef>>([]);
  const [filters, setFilters] = useState<ReadonlyArray<KeywordFilter>>([]);
  const [domainBlocks, setDomainBlocks] = useState<ReadonlyArray<string>>([]);
  const [followedTags, setFollowedTags] = useState<ReadonlyArray<FollowedTag>>([]);
  const [filterTitle, setFilterTitle] = useState("");
  const [filterKeywords, setFilterKeywords] = useState("");
  const [domainInput, setDomainInput] = useState("");

  const [displayName, setDisplayName] = useState("");
  const [note, setNote] = useState("");
  const [defaultVisibility, setDefaultVisibility] = useState("public");
  const [defaultSensitive, setDefaultSensitive] = useState(false);
  const [defaultLanguage, setDefaultLanguage] = useState("");
  const [defaultQuotePolicy, setDefaultQuotePolicy] = useState("public");

  const [savingProfile, setSavingProfile] = useState(false);
  const [savingPosting, setSavingPosting] = useState(false);
  const [savingPolicy, setSavingPolicy] = useState(false);
  const [savingModeration, setSavingModeration] = useState(false);
  const [sectionMessage, setSectionMessage] = useState("");

  const loadSettings = useCallback(async () => {
    const [
      credentialsResult,
      preferencesResult,
      policyResult,
      mutesResult,
      blocksResult,
      filtersResult,
      domainsResult,
      tagsResult,
    ] = await Promise.all([
        fetchAccountCredentials(),
        fetchAccountPreferences(),
        fetchNotificationPolicy(),
        fetchMutedAccounts(),
        fetchBlockedAccounts(),
        fetchKeywordFilters(),
        fetchDomainBlocks(),
        fetchFollowedTags(),
      ]);

    if (credentialsResult.isErr()) {
      throw new Error(mastodonErrorMessage(credentialsResult.error));
    }
    if (preferencesResult.isErr()) {
      throw new Error(mastodonErrorMessage(preferencesResult.error));
    }
    if (policyResult.isErr()) {
      throw new Error(mastodonErrorMessage(policyResult.error));
    }
    if (mutesResult.isErr()) {
      throw new Error(mastodonErrorMessage(mutesResult.error));
    }
    if (blocksResult.isErr()) {
      throw new Error(mastodonErrorMessage(blocksResult.error));
    }
    if (filtersResult.isErr()) {
      throw new Error(mastodonErrorMessage(filtersResult.error));
    }
    if (domainsResult.isErr()) {
      throw new Error(mastodonErrorMessage(domainsResult.error));
    }
    if (tagsResult.isErr()) {
      throw new Error(mastodonErrorMessage(tagsResult.error));
    }

    setCredentials(credentialsResult.value);
    setNotificationPolicy(policyResult.value);
    setMutes(mutesResult.value);
    setBlocks(blocksResult.value);
    setFilters(filtersResult.value);
    setDomainBlocks(domainsResult.value);
    setFollowedTags(tagsResult.value);

    setDisplayName(credentialsResult.value.displayName);
    setNote(credentialsResult.value.source.note);
    setDefaultVisibility(preferencesResult.value.defaultVisibility);
    setDefaultSensitive(preferencesResult.value.defaultSensitive);
    setDefaultLanguage(preferencesResult.value.defaultLanguage ?? "");
    setDefaultQuotePolicy(preferencesResult.value.defaultQuotePolicy);
  }, []);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setError("");
    void loadSettings()
      .catch((loadError) => {
        if (active) {
          setError(loadError instanceof Error ? loadError.message : "設定の読み込みに失敗しました");
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
  }, [loadSettings]);

  const syncSessionAccount = (updated: AccountCredentials) => {
    if (session.kind !== "Authenticated") {
      return;
    }
    setSession(
      SessionState.updateAccount(session, {
        ...session.account,
        displayName: updated.displayName,
        avatar: updated.avatar,
      }),
    );
  };

  const handleSaveProfile = async () => {
    setSavingProfile(true);
    setSectionMessage("");
    try {
      const result = await updateAccountProfile({ displayName: displayName.trim(), note });
      if (result.isErr()) {
        throw new Error(mastodonErrorMessage(result.error));
      }
      setCredentials(result.value);
      syncSessionAccount(result.value);
      setSectionMessage("プロフィールを保存しました");
    } catch (saveError) {
      setSectionMessage(saveError instanceof Error ? saveError.message : "保存に失敗しました");
    } finally {
      setSavingProfile(false);
    }
  };

  const handleSavePosting = async () => {
    setSavingPosting(true);
    setSectionMessage("");
    try {
      const result = await updatePostingPreferences({
        defaultVisibility,
        defaultSensitive,
        defaultLanguage: defaultLanguage.trim() || null,
        defaultQuotePolicy,
      });
      if (result.isErr()) {
        throw new Error(mastodonErrorMessage(result.error));
      }
      setCredentials(result.value);
      setDefaultVisibility(result.value.source.privacy);
      setDefaultSensitive(result.value.source.sensitive);
      setDefaultLanguage(result.value.source.language ?? "");
      setDefaultQuotePolicy(result.value.source.quotePolicy);
      setSectionMessage("投稿設定を保存しました");
    } catch (saveError) {
      setSectionMessage(saveError instanceof Error ? saveError.message : "保存に失敗しました");
    } finally {
      setSavingPosting(false);
    }
  };

  const handlePolicyChange = async (
    field: keyof NotificationPolicy,
    value: NotificationPolicyAction,
  ) => {
    if (!notificationPolicy) {
      return;
    }
    setSavingPolicy(true);
    setSectionMessage("");
    const previous = notificationPolicy;
    setNotificationPolicy({ ...notificationPolicy, [field]: value });
    try {
      const result = await updateNotificationPolicy({ [field]: value });
      if (result.isErr()) {
        throw new Error(mastodonErrorMessage(result.error));
      }
      setNotificationPolicy(result.value);
      setSectionMessage("通知ポリシーを更新しました");
    } catch (saveError) {
      setNotificationPolicy(previous);
      setSectionMessage(saveError instanceof Error ? saveError.message : "更新に失敗しました");
    } finally {
      setSavingPolicy(false);
    }
  };

  const handleUnmute = async (accountId: string) => {
    setSavingModeration(true);
    setSectionMessage("");
    const result = await unmuteAccount(accountId);
    setSavingModeration(false);
    if (result.isErr()) {
      setSectionMessage(mastodonErrorMessage(result.error));
      return;
    }
    setMutes((current) => current.filter((account) => account.id !== accountId));
    setSectionMessage("ミュートを解除しました");
  };

  const handleUnblock = async (accountId: string) => {
    setSavingModeration(true);
    setSectionMessage("");
    const result = await unblockAccount(accountId);
    setSavingModeration(false);
    if (result.isErr()) {
      setSectionMessage(mastodonErrorMessage(result.error));
      return;
    }
    setBlocks((current) => current.filter((account) => account.id !== accountId));
    setSectionMessage("ブロックを解除しました");
  };

  const handleCreateFilter = async () => {
    const title = filterTitle.trim();
    const keywords = filterKeywords
      .split(/[,\n]/)
      .map((keyword) => keyword.trim())
      .filter((keyword) => keyword.length > 0);
    if (!title || keywords.length === 0) {
      setSectionMessage("タイトルとキーワードを入力してください");
      return;
    }
    setSavingModeration(true);
    const result = await createKeywordFilter({
      title,
      context: [...FilterContext.values],
      keywords,
      filterAction: "warn",
    });
    setSavingModeration(false);
    if (result.isErr()) {
      setSectionMessage(mastodonErrorMessage(result.error));
      return;
    }
    setFilters((current) => [result.value, ...current]);
    setFilterTitle("");
    setFilterKeywords("");
    setSectionMessage("フィルターを追加しました");
  };

  const handleDeleteFilter = async (filterId: string) => {
    setSavingModeration(true);
    const result = await deleteKeywordFilter(filterId);
    setSavingModeration(false);
    if (result.isErr()) {
      setSectionMessage(mastodonErrorMessage(result.error));
      return;
    }
    setFilters((current) => current.filter((item) => item.id !== filterId));
    setSectionMessage("フィルターを削除しました");
  };

  const handleBlockDomain = async () => {
    const domain = domainInput.trim().toLowerCase();
    if (!domain) {
      return;
    }
    setSavingModeration(true);
    const result = await blockDomain(domain);
    setSavingModeration(false);
    if (result.isErr()) {
      setSectionMessage(mastodonErrorMessage(result.error));
      return;
    }
    setDomainBlocks((current) => (current.includes(domain) ? current : [domain, ...current]));
    setDomainInput("");
    setSectionMessage("ドメインをブロックしました");
  };

  const handleUnblockDomain = async (domain: string) => {
    setSavingModeration(true);
    const result = await unblockDomain(domain);
    setSavingModeration(false);
    if (result.isErr()) {
      setSectionMessage(mastodonErrorMessage(result.error));
      return;
    }
    setDomainBlocks((current) => current.filter((item) => item !== domain));
    setSectionMessage("ドメインブロックを解除しました");
  };

  const handleUnfollowTag = async (name: string) => {
    setSavingModeration(true);
    const result = await unfollowTag(name);
    setSavingModeration(false);
    if (result.isErr()) {
      setSectionMessage(mastodonErrorMessage(result.error));
      return;
    }
    setFollowedTags((current) => current.filter((tag) => tag.name !== name));
    setSectionMessage("ハッシュタグのフォローを解除しました");
  };

  const handleLogout = () => {
    clearSession();
    window.location.assign("/app/logout");
  };

  return (
    <AppShell title="設定">
      {loading ? <div className="app-status">読み込み中…</div> : null}
      {error ? <p className="app-error">{error}</p> : null}
      {sectionMessage ? <p className="app-status">{sectionMessage}</p> : null}

      {!loading && !error ? (
        <>
          <section className="app-card settings-section" data-phase={WebUiPhase.settings}>
            <h2>アカウント</h2>
            <div className="settings-form">
              <label className="settings-field">
                <span>表示名</span>
                <input
                  value={displayName}
                  onChange={(event) => setDisplayName(event.target.value)}
                  disabled={savingProfile}
                />
              </label>
              <label className="settings-field">
                <span>自己紹介</span>
                <textarea
                  value={note}
                  onChange={(event) => setNote(event.target.value)}
                  rows={4}
                  disabled={savingProfile}
                />
              </label>
              <p className="app-muted">
                アイコンと背景画像は{" "}
                <Link to="/profile">プロフィールページ</Link>
                から変更できます。
              </p>
              <button
                type="button"
                className="app-button"
                onClick={() => void handleSaveProfile()}
                disabled={savingProfile}
              >
                {savingProfile ? "保存中…" : "プロフィールを保存"}
              </button>
            </div>
          </section>

          <section className="app-card settings-section" data-phase={WebUiPhase.settings}>
            <h2>投稿の既定値</h2>
            <div className="settings-form">
              <label className="settings-field">
                <span>公開範囲</span>
                <select
                  value={defaultVisibility}
                  onChange={(event) => setDefaultVisibility(event.target.value)}
                  disabled={savingPosting}
                >
                  {VISIBILITY_OPTIONS.map((option) => (
                    <option key={option.kind} value={Visibility.toApi(option)}>
                      {Visibility.label(option)}
                    </option>
                  ))}
                </select>
              </label>
              <label className="settings-field settings-field-inline">
                <input
                  type="checkbox"
                  checked={defaultSensitive}
                  onChange={(event) => setDefaultSensitive(event.target.checked)}
                  disabled={savingPosting}
                />
                <span>デフォルトで CW を付ける</span>
              </label>
              <label className="settings-field">
                <span>言語（ISO 639-1）</span>
                <input
                  value={defaultLanguage}
                  onChange={(event) => setDefaultLanguage(event.target.value)}
                  placeholder="ja"
                  disabled={savingPosting}
                />
              </label>
              <label className="settings-field">
                <span>引用の許可</span>
                <select
                  value={defaultQuotePolicy}
                  onChange={(event) => setDefaultQuotePolicy(event.target.value)}
                  disabled={savingPosting}
                >
                  {QUOTE_POLICY_OPTIONS.map((option) => (
                    <option key={option.value} value={option.value}>
                      {option.label}
                    </option>
                  ))}
                </select>
              </label>
              <button
                type="button"
                className="app-button"
                onClick={() => void handleSavePosting()}
                disabled={savingPosting}
              >
                {savingPosting ? "保存中…" : "投稿設定を保存"}
              </button>
            </div>
          </section>

          <section className="app-card settings-section" data-phase={WebUiPhase.settings}>
            <h2>通知ポリシー</h2>
            <div className="settings-form">
              {notificationPolicy
                ? POLICY_FIELDS.map(({ key, label }) => (
                    <label key={key} className="settings-field">
                      <span>{label}</span>
                      <select
                        value={notificationPolicy[key]}
                        onChange={(event) =>
                          void handlePolicyChange(key, event.target.value as NotificationPolicyAction)
                        }
                        disabled={savingPolicy}
                      >
                        {(["accept", "filter", "drop"] as const).map((action) => (
                          <option key={action} value={action}>
                            {NotificationPolicyModel.actionLabel(action)}
                          </option>
                        ))}
                      </select>
                    </label>
                  ))
                : null}
            </div>
          </section>

          <section className="app-card settings-section" data-phase={WebUiPhase.settings}>
            <h2>キーワードフィルター</h2>
            <div className="settings-form">
              <label className="settings-field">
                <span>タイトル</span>
                <input
                  value={filterTitle}
                  onChange={(event) => setFilterTitle(event.target.value)}
                  disabled={savingModeration}
                />
              </label>
              <label className="settings-field">
                <span>キーワード（カンマ区切り）</span>
                <input
                  value={filterKeywords}
                  onChange={(event) => setFilterKeywords(event.target.value)}
                  disabled={savingModeration}
                />
              </label>
              <button
                type="button"
                className="app-button"
                onClick={() => void handleCreateFilter()}
                disabled={savingModeration}
              >
                追加
              </button>
            </div>
            {filters.length === 0 ? (
              <p className="app-muted">キーワードフィルターはありません</p>
            ) : (
              <div className="settings-account-list">
                {filters.map((item) => (
                  <div key={item.id} className="settings-moderation-row">
                    <div>
                      <strong>{item.title}</strong>
                      <p className="app-muted">
                        {item.keywords.map((keyword) => keyword.keyword).join(", ")}
                      </p>
                    </div>
                    <button
                      type="button"
                      className="app-button app-button-secondary"
                      disabled={savingModeration}
                      onClick={() => void handleDeleteFilter(item.id)}
                    >
                      削除
                    </button>
                  </div>
                ))}
              </div>
            )}
          </section>

          <section className="app-card settings-section" data-phase={WebUiPhase.settings}>
            <h2>ドメインブロック</h2>
            <div className="settings-form">
              <label className="settings-field">
                <span>ドメイン</span>
                <input
                  value={domainInput}
                  onChange={(event) => setDomainInput(event.target.value)}
                  placeholder="example.com"
                  disabled={savingModeration}
                />
              </label>
              <button
                type="button"
                className="app-button"
                onClick={() => void handleBlockDomain()}
                disabled={savingModeration}
              >
                ブロック
              </button>
            </div>
            {domainBlocks.length === 0 ? (
              <p className="app-muted">ブロック中のドメインはありません</p>
            ) : (
              <div className="settings-account-list">
                {domainBlocks.map((domain) => (
                  <div key={domain} className="settings-moderation-row">
                    <span>{domain}</span>
                    <button
                      type="button"
                      className="app-button app-button-secondary"
                      disabled={savingModeration}
                      onClick={() => void handleUnblockDomain(domain)}
                    >
                      解除
                    </button>
                  </div>
                ))}
              </div>
            )}
          </section>

          <section className="app-card settings-section" data-phase={WebUiPhase.settings}>
            <h2>フォロー中のハッシュタグ</h2>
            {followedTags.length === 0 ? (
              <p className="app-muted">フォロー中のハッシュタグはありません</p>
            ) : (
              <div className="settings-account-list">
                {followedTags.map((tag) => (
                  <div key={tag.id} className="settings-moderation-row">
                    <Link to={`/tags/${encodeURIComponent(tag.name)}`}>#{tag.name}</Link>
                    <button
                      type="button"
                      className="app-button app-button-secondary"
                      disabled={savingModeration}
                      onClick={() => void handleUnfollowTag(tag.name)}
                    >
                      解除
                    </button>
                  </div>
                ))}
              </div>
            )}
          </section>

          <section className="app-card settings-section" data-phase={WebUiPhase.settings}>
            <h2>ミュートとブロック</h2>
            <div className="settings-moderation">
              <div>
                <h3>ミュート ({mutes.length})</h3>
                {mutes.length === 0 ? (
                  <p className="app-muted">ミュート中のアカウントはありません</p>
                ) : (
                  <div className="settings-account-list">
                    {mutes.map((account) => (
                      <ModerationAccountRow
                        key={account.id}
                        account={account}
                        actionLabel="解除"
                        disabled={savingModeration}
                        onAction={() => void handleUnmute(account.id)}
                      />
                    ))}
                  </div>
                )}
              </div>
              <div>
                <h3>ブロック ({blocks.length})</h3>
                {blocks.length === 0 ? (
                  <p className="app-muted">ブロック中のアカウントはありません</p>
                ) : (
                  <div className="settings-account-list">
                    {blocks.map((account) => (
                      <ModerationAccountRow
                        key={account.id}
                        account={account}
                        actionLabel="解除"
                        disabled={savingModeration}
                        onAction={() => void handleUnblock(account.id)}
                      />
                    ))}
                  </div>
                )}
              </div>
            </div>
          </section>

          <section className="app-card settings-section" data-phase={WebUiPhase.collections}>
            <h2>ライブラリ</h2>
            <p className="app-muted">モバイルでもコレクションへ移動できます。</p>
            <div className="settings-library-links">
              {[AppRoute.explore(), AppRoute.bookmarks(), AppRoute.favourites(), AppRoute.lists(), AppRoute.messages()].map((route) => {
                const isMessages = route.kind === "Messages";
                const label = AppRoute.label(route);
                return (
                  <Link
                    key={route.kind}
                    className="app-button app-button-secondary settings-library-link"
                    to={AppRoute.toPath(route)}
                    aria-label={
                      isMessages && unreadCount > 0 ? `${label}（未読 ${unreadCount}）` : label
                    }
                  >
                    <span>{label}</span>
                    {isMessages && unreadCount > 0 ? (
                      <span className="nav-unread-badge" aria-hidden="true">
                        {unreadCount > 99 ? "99+" : unreadCount}
                      </span>
                    ) : null}
                  </Link>
                );
              })}
            </div>
          </section>

          <section className="app-card settings-section" data-phase={WebUiPhase.settings}>
            <h2>ホーム画面</h2>
            {installed ? (
              <p className="app-muted">アプリとして起動しています。</p>
            ) : (
              <>
                <p className="app-muted">
                  ホーム画面に追加すると、ブラウザの枠なしで cfwdon を使えます。
                </p>
                {canInstall ? (
                  <button type="button" className="app-button" onClick={() => void install()}>
                    インストール
                  </button>
                ) : (
                  <p className="app-muted">
                    iPhone / iPad では共有ボタンから「ホーム画面に追加」を選んでください。
                  </p>
                )}
              </>
            )}
          </section>

          <section className="app-card settings-section" data-phase={WebUiPhase.settings}>
            <h2>セッション</h2>
            {credentials ? (
              <p className="app-muted">
                ログイン中: @{credentials.acct}
                {session.kind === "Authenticated" ? ` (${session.account.instanceName})` : null}
              </p>
            ) : null}
            <button type="button" className="app-button app-button-secondary" onClick={handleLogout}>
              ログアウト
            </button>
          </section>
        </>
      ) : null}
    </AppShell>
  );
};
