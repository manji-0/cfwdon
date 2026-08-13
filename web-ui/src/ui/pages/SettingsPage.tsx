import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { AccountCredentials } from "@/domain/account/credentials";
import type { AccountRef } from "@/domain/account/account";
import type { NotificationPolicy, NotificationPolicyAction } from "@/domain/settings/notification-policy";
import { NotificationPolicy as NotificationPolicyModel } from "@/domain/settings/notification-policy";
import { SessionState } from "@/domain/session/session";
import { Visibility } from "@/domain/status/visibility";
import { AppRoute } from "@/domain/navigation/route";
import { fetchAccountCredentials, updateAccountProfile } from "@/infrastructure/api/credentials";
import { fetchBlockedAccounts, fetchMutedAccounts } from "@/infrastructure/api/moderation";
import {
  fetchNotificationPolicy,
  updateNotificationPolicy,
} from "@/infrastructure/api/notification-policy";
import { fetchAccountPreferences, updatePostingPreferences } from "@/infrastructure/api/settings";
import { WebUiPhase } from "@/plan/phases";
import { AppShell } from "@/ui/components/AppShell";
import { useSession } from "@/ui/context/SessionContext";
import { useUnreadMessages } from "@/ui/context/UnreadMessagesContext";

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

const ModerationAccountLink = ({ account }: Readonly<{ account: AccountRef }>) => (
  <Link className="account-row" to={`/profile/${account.id}`}>
    <img className="status-avatar" src={account.avatar} alt="" loading="lazy" />
    <div className="account-row-meta">
      <span className="status-display-name">{account.displayName || account.username}</span>
      <span className="status-acct">@{account.acct}</span>
    </div>
  </Link>
);

export const SettingsPage = () => {
  const { session, setSession, clearSession } = useSession();
  const { unreadCount } = useUnreadMessages();

  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  const [credentials, setCredentials] = useState<AccountCredentials | null>(null);
  const [notificationPolicy, setNotificationPolicy] = useState<NotificationPolicy | null>(null);
  const [mutes, setMutes] = useState<ReadonlyArray<AccountRef>>([]);
  const [blocks, setBlocks] = useState<ReadonlyArray<AccountRef>>([]);

  const [displayName, setDisplayName] = useState("");
  const [note, setNote] = useState("");
  const [defaultVisibility, setDefaultVisibility] = useState("public");
  const [defaultSensitive, setDefaultSensitive] = useState(false);
  const [defaultLanguage, setDefaultLanguage] = useState("");
  const [defaultQuotePolicy, setDefaultQuotePolicy] = useState("public");

  const [savingProfile, setSavingProfile] = useState(false);
  const [savingPosting, setSavingPosting] = useState(false);
  const [savingPolicy, setSavingPolicy] = useState(false);
  const [sectionMessage, setSectionMessage] = useState("");

  const loadSettings = useCallback(async () => {
    const [credentialsResult, preferencesResult, policyResult, mutesResult, blocksResult] =
      await Promise.all([
        fetchAccountCredentials(),
        fetchAccountPreferences(),
        fetchNotificationPolicy(),
        fetchMutedAccounts(),
        fetchBlockedAccounts(),
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

    setCredentials(credentialsResult.value);
    setNotificationPolicy(policyResult.value);
    setMutes(mutesResult.value);
    setBlocks(blocksResult.value);

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
      SessionState.authenticated({
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
            <h2>フィルター</h2>
            <div className="settings-moderation">
              <div>
                <h3>ミュート ({mutes.length})</h3>
                {mutes.length === 0 ? (
                  <p className="app-muted">ミュート中のアカウントはありません</p>
                ) : (
                  <div className="settings-account-list">
                    {mutes.map((account) => (
                      <ModerationAccountLink key={account.id} account={account} />
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
                      <ModerationAccountLink key={account.id} account={account} />
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
              {[AppRoute.bookmarks(), AppRoute.lists(), AppRoute.messages()].map((route) => {
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
