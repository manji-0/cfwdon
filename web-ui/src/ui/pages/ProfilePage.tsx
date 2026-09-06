import { useEffect, useRef, useState } from "react";
import { Link, useParams } from "react-router";
import { loadProfileSnapshot } from "@/application/load-profile-snapshot";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { AccountProfile } from "@/domain/account/account";
import { Relationship } from "@/domain/account/relationship";
import { CachedView } from "@/domain/cache/cached-view";
import { ViewReadiness } from "@/domain/cache/view-readiness";
import { Status } from "@/domain/status/status";
import type { FeaturedTag } from "@/domain/tags/featured-tag";
import { fetchAccountStatuses } from "@/infrastructure/api/account";
import {
  blockAccount,
  fetchRelationship,
  followAccount,
  muteAccount,
  unfollowAccount,
  unmuteAccount,
  unblockAccount,
} from "@/infrastructure/api/relationship";
import { createReport } from "@/infrastructure/api/report";
import { fetchAccountFeaturedTags } from "@/infrastructure/api/tags";
import { AppShell } from "@/ui/components/AppShell";
import { LoadMoreFooter } from "@/ui/components/LoadMoreFooter";
import { ProfileEditor } from "@/ui/components/ProfileEditor";
import { StatusCard } from "@/ui/components/StatusCard";
import { useConfirm } from "@/ui/context/ConfirmContext";
import { useSession } from "@/ui/context/SessionContext";
import { useViewCache } from "@/ui/context/ViewCacheContext";
import { useStatusActions } from "@/ui/hooks/useStatusActions";
import { usePagePrefetch } from "@/ui/hooks/usePagePrefetch";
import { useWindowScrollY } from "@/ui/hooks/useWindowScrollY";
import { TIMELINE_PAGE_LIMIT, pageHasMore } from "@/ui/lib/pagination";

const PROFILE_TABS = [
  { id: "posts", label: "投稿" },
  { id: "replies", label: "返信" },
  { id: "media", label: "メディア" },
  { id: "pinned", label: "固定" },
] as const;

type ProfileTab = (typeof PROFILE_TABS)[number]["id"];

const statusesQueryForTab = (tab: ProfileTab) => {
  switch (tab) {
    case "posts":
      return { excludeReplies: true };
    case "replies":
      return {};
    case "media":
      return { onlyMedia: true };
    case "pinned":
      return { pinned: true };
  }
};

const emptyMessageForTab = (tab: ProfileTab): string => {
  switch (tab) {
    case "posts":
      return "まだ投稿がありません。";
    case "replies":
      return "返信はまだありません。";
    case "media":
      return "メディア付きの投稿はまだありません。";
    case "pinned":
      return "ピン留めされた投稿はありません。";
  }
};

export const ProfilePage = () => {
  const { accountId: routeAccountId } = useParams();
  const { session } = useSession();
  const { prompt, alert } = useConfirm();
  const cache = useViewCache();
  const selfAccountId = session.kind === "Authenticated" ? session.account.id : null;
  const accountId = routeAccountId ?? selfAccountId;
  const isSelf = Boolean(accountId && selfAccountId && accountId === selfAccountId);
  const cached = accountId ? cache.getProfile(accountId) : CachedView.absent();

  const [profile, setProfile] = useState<AccountProfile | null>(
    cached.kind === "Present" ? cached.value.profile : null,
  );
  const [statuses, setStatuses] = useState<ReadonlyArray<Status>>(
    cached.kind === "Present" ? cached.value.statuses : [],
  );
  const [loading, setLoading] = useState(CachedView.isAbsent(cached));
  const [loadingMore, setLoadingMore] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const [editing, setEditing] = useState(false);
  const [error, setError] = useState("");
  const [relationship, setRelationship] = useState<Relationship | null>(null);
  const [savingRelationship, setSavingRelationship] = useState(false);
  const [tab, setTab] = useState<ProfileTab>("posts");
  const [featuredTags, setFeaturedTags] = useState<ReadonlyArray<FeaturedTag>>([]);
  const tabRef = useRef(tab);
  tabRef.current = tab;
  const fetchedAtRef = useRef(cached.kind === "Present" ? cached.value.fetchedAt : 0);
  const profileRef = useRef(profile);
  const statusesRef = useRef(statuses);
  const loadingMoreRef = useRef(false);
  const scrollYRef = useWindowScrollY();
  profileRef.current = profile;
  statusesRef.current = statuses;
  const prefetch = usePagePrefetch(async (maxId: string) => {
    if (!accountId) {
      return [];
    }
    const result = await fetchAccountStatuses(accountId, {
      maxId,
      limit: TIMELINE_PAGE_LIMIT,
      ...statusesQueryForTab(tabRef.current),
    });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    return result.value;
  });

  useEffect(() => {
    if (!accountId) {
      return undefined;
    }
    const snapshot = cache.getProfile(accountId);
    if (snapshot.kind === "Present") {
      setProfile(snapshot.value.profile);
      setStatuses(snapshot.value.statuses);
      fetchedAtRef.current = snapshot.value.fetchedAt;
      setHasMore(pageHasMore(snapshot.value.statuses.length));
      setLoading(false);
      prefetch.prepareNext(snapshot.value.statuses, snapshot.value.statuses.length);
      requestAnimationFrame(() => window.scrollTo(0, snapshot.value.scrollY));
    } else {
      setProfile(null);
      setStatuses([]);
      fetchedAtRef.current = 0;
      setHasMore(true);
      prefetch.reset();
      setLoading(true);
    }
    setEditing(false);
    setError("");
    setRelationship(null);
    setTab("posts");
    setFeaturedTags([]);

    let active = true;
    switch (ViewReadiness.forProfile(snapshot, Date.now()).kind) {
      case "Skip":
        break;
      case "Load":
      case "Revalidate":
        void Promise.resolve(loadProfileSnapshot(accountId))
          .then((result) => {
            if (!active) {
              return;
            }
            if (result.isErr()) {
              throw new Error(mastodonErrorMessage(result.error));
            }
            fetchedAtRef.current = result.value.fetchedAt;
            setProfile(result.value.profile);
            setStatuses(result.value.statuses);
            setHasMore(pageHasMore(result.value.statuses.length));
            prefetch.prepareNext(result.value.statuses, result.value.statuses.length);
            cache.writeProfile(accountId, {
              ...result.value,
              scrollY: scrollYRef.current,
            });
          })
          .catch((loadError) => {
            if (active) {
              setError(loadError instanceof Error ? loadError.message : "プロフィールの読み込みに失敗しました");
            }
          })
          .finally(() => {
            if (active) {
              setLoading(false);
            }
          });
        break;
    }

    return () => {
      active = false;
      const currentProfile =
        profileRef.current ?? (snapshot.kind === "Present" ? snapshot.value.profile : null);
      if (!currentProfile || fetchedAtRef.current === 0) {
        return;
      }
      cache.writeProfile(accountId, {
        profile: currentProfile,
        statuses: statusesRef.current,
        fetchedAt: fetchedAtRef.current,
        scrollY: scrollYRef.current,
      });
    };
  }, [accountId, cache, prefetch]);

  useEffect(() => {
    if (!accountId || isSelf) {
      setRelationship(null);
      return;
    }
    let active = true;
    void fetchRelationship(accountId).then((result) => {
      if (active && result.isOk()) {
        setRelationship(result.value);
      }
    });
    return () => {
      active = false;
    };
  }, [accountId, isSelf]);

  useEffect(() => {
    if (!accountId) {
      setFeaturedTags([]);
      return;
    }
    let active = true;
    void fetchAccountFeaturedTags(accountId).then((result) => {
      if (active && result.isOk()) {
        setFeaturedTags(result.value);
      }
    });
    return () => {
      active = false;
    };
  }, [accountId]);

  useEffect(() => {
    if (!accountId || tab === "posts") {
      return;
    }
    let active = true;
    setLoading(true);
    prefetch.reset();
    void fetchAccountStatuses(accountId, statusesQueryForTab(tab)).then((result) => {
      if (!active) {
        return;
      }
      setLoading(false);
      if (result.isErr()) {
        setError(mastodonErrorMessage(result.error));
        return;
      }
      setStatuses(result.value);
      setHasMore(pageHasMore(result.value.length));
      prefetch.prepareNext(result.value, result.value.length);
    });
    return () => {
      active = false;
    };
  }, [accountId, tab, prefetch]);

  const handleLoadMore = async () => {
    if (!accountId) {
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

  const actions = useStatusActions({
    selfAccountId,
    onReplace: (updated) => {
      setStatuses((current) => Status.replaceInList(current, updated));
      cache.patchStatus(updated);
    },
    onRemove: (statusId) => setStatuses((current) => Status.removeById(current, statusId)),
    onError: setError,
  });

  const runRelationship = async (action: () => ReturnType<typeof followAccount>) => {
    setSavingRelationship(true);
    const result = await action();
    setSavingRelationship(false);
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    setRelationship(result.value);
  };

  const handleFollowToggle = async () => {
    if (!accountId || !relationship) {
      return;
    }
    if (relationship.following || relationship.requested) {
      await runRelationship(() => unfollowAccount(accountId));
      return;
    }
    await runRelationship(() => followAccount(accountId));
  };

  const handleReblogsToggle = async () => {
    if (!accountId || !relationship?.following) {
      return;
    }
    await runRelationship(() =>
      followAccount(accountId, { reblogs: !relationship.showingReblogs }),
    );
  };

  const handleMuteToggle = async () => {
    if (!accountId || !relationship) {
      return;
    }
    await runRelationship(() =>
      relationship.muting ? unmuteAccount(accountId) : muteAccount(accountId),
    );
  };

  const handleBlockToggle = async () => {
    if (!accountId || !relationship) {
      return;
    }
    await runRelationship(() =>
      relationship.blocking ? unblockAccount(accountId) : blockAccount(accountId),
    );
  };

  const handleReportProfile = async () => {
    if (!accountId) {
      return;
    }
    const comment = await prompt("通報の理由（任意）", {
      title: "通報",
      confirmLabel: "送信",
    });
    if (comment === null) {
      return;
    }
    const result = await createReport({ accountId, comment });
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    await alert("通報を送信しました", { title: "通報" });
  };

  if (!accountId) {
    return (
      <AppShell title="プロフィール">
        <div className="app-card">
          <p className="app-muted">ログインするとプロフィールを表示できます。</p>
          <Link className="app-button" to="/login">
            ログイン
          </Link>
        </div>
      </AppShell>
    );
  }

  return (
    <AppShell title={profile?.displayName || profile?.username || "プロフィール"}>
      {error ? <p className="app-error">{error}</p> : null}
      {loading ? <div className="app-status">読み込み中…</div> : null}
      {profile && editing ? (
        <header className="profile-header">
          <ProfileEditor
            profile={profile}
            onCancel={() => setEditing(false)}
            onSaved={(updated) => {
              setProfile(updated);
              setEditing(false);
              cache.writeProfile(accountId, {
                profile: updated,
                statuses: statusesRef.current,
                fetchedAt: Date.now(),
                scrollY: scrollYRef.current,
              });
            }}
          />
        </header>
      ) : null}
      {profile && !editing ? (
        <header className="profile-header">
          <div
            className="profile-header-banner"
            style={{ backgroundImage: profile.header ? `url(${profile.header})` : undefined }}
          />
          <div className="profile-header-body">
            <img className="profile-avatar" src={profile.avatar} alt="" />
            <div className="profile-heading">
              <h1 className="profile-name">{profile.displayName || profile.username}</h1>
              <p className="profile-acct">
                @{profile.acct}
                {profile.locked ? <span className="profile-badge">鍵</span> : null}
                {profile.bot ? <span className="profile-badge">bot</span> : null}
              </p>
              {isSelf ? (
                <button
                  type="button"
                  className="app-button app-button-secondary profile-edit-button"
                  onClick={() => setEditing(true)}
                >
                  プロフィールを編集
                </button>
              ) : (
                <div className="profile-actions">
                  <button
                    type="button"
                    className="app-button"
                    disabled={savingRelationship || !relationship}
                    onClick={() => void handleFollowToggle()}
                  >
                    {relationship
                      ? Relationship.followLabel(relationship, profile.locked)
                      : "フォロー"}
                  </button>
                  {relationship?.following ? (
                    <button
                      type="button"
                      className="app-button app-button-secondary"
                      disabled={savingRelationship}
                      onClick={() => void handleReblogsToggle()}
                    >
                      {relationship.showingReblogs ? "ブーストを非表示" : "ブーストを表示"}
                    </button>
                  ) : null}
                  <button
                    type="button"
                    className="app-button app-button-secondary"
                    disabled={savingRelationship || !relationship}
                    onClick={() => void handleMuteToggle()}
                  >
                    {relationship?.muting ? "ミュート解除" : "ミュート"}
                  </button>
                  <button
                    type="button"
                    className="app-button app-button-secondary"
                    disabled={savingRelationship || !relationship}
                    onClick={() => void handleBlockToggle()}
                  >
                    {relationship?.blocking ? "ブロック解除" : "ブロック"}
                  </button>
                  <button
                    type="button"
                    className="app-button app-button-secondary"
                    onClick={() => void handleReportProfile()}
                  >
                    通報
                  </button>
                </div>
              )}
            </div>
            {profile.note ? (
              <div className="profile-note" dangerouslySetInnerHTML={{ __html: profile.note }} />
            ) : null}
            {profile.fields.length > 0 ? (
              <dl className="profile-fields">
                {profile.fields.map((field) => (
                  <div key={`${field.name}:${field.value}`} className="profile-field">
                    <dt>{field.name}</dt>
                    <dd dangerouslySetInnerHTML={{ __html: field.value }} />
                  </div>
                ))}
              </dl>
            ) : null}
            {featuredTags.length > 0 ? (
              <ul className="profile-featured-tags">
                {featuredTags.map((tag) => (
                  <li key={tag.id}>
                    <Link to={`/tags/${encodeURIComponent(tag.name)}`}>#{tag.name}</Link>
                    <span className="app-muted">{tag.statusesCount}</span>
                  </li>
                ))}
              </ul>
            ) : null}
            <p className="profile-stats app-muted">
              <span>{profile.statusesCount} 投稿</span>
              <Link to={`/profile/${profile.id}/following`}>{profile.followingCount} フォロー</Link>
              <Link to={`/profile/${profile.id}/followers`}>
                {profile.followersCount} フォロワー
              </Link>
            </p>
          </div>
        </header>
      ) : null}
      <nav className="timeline-tabs" aria-label="プロフィール投稿">
        {PROFILE_TABS.map((item) => (
          <button
            key={item.id}
            type="button"
            className={tab === item.id ? "is-active" : undefined}
            onClick={() => {
              if (item.id === tab) {
                return;
              }
              setTab(item.id);
              tabRef.current = item.id;
              if (item.id === "posts" && accountId) {
                const snapshot = cache.getProfile(accountId);
                if (snapshot.kind === "Present") {
                  setStatuses(snapshot.value.statuses);
                  setHasMore(pageHasMore(snapshot.value.statuses.length));
                  prefetch.prepareNext(snapshot.value.statuses, snapshot.value.statuses.length);
                }
              }
            }}
          >
            {item.label}
          </button>
        ))}
      </nav>
      <div className="timeline">
        {statuses.map((status) => (
          <StatusCard key={status.id} status={status} {...actions} />
        ))}
      </div>
      {!loading && statuses.length === 0 ? (
        <div className="app-card">
          <p className="app-muted">{emptyMessageForTab(tab)}</p>
        </div>
      ) : null}
      <LoadMoreFooter
        hasMore={hasMore && !loading && statuses.length > 0}
        loading={loadingMore}
        observeKey={statuses.length}
        onLoadMore={() => void handleLoadMore()}
      />
    </AppShell>
  );
};
