import { useEffect, useRef, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { AccountProfile } from "@/domain/account/account";
import { ViewCache } from "@/domain/cache/view-cache";
import type { Status } from "@/domain/status/status";
import { Status as StatusModel } from "@/domain/status/status";
import {
  bookmarkStatus,
  favouriteStatus,
  reblogStatus,
  unbookmarkStatus,
  unfavouriteStatus,
  unreblogStatus,
} from "@/infrastructure/api/status";
import { fetchAccountProfile, fetchAccountStatuses } from "@/infrastructure/api/account";
import { AppShell } from "@/ui/components/AppShell";
import { ProfileEditor } from "@/ui/components/ProfileEditor";
import { StatusCard } from "@/ui/components/StatusCard";
import { useSession } from "@/ui/context/SessionContext";
import { useViewCache } from "@/ui/context/ViewCacheContext";
import { useWindowScrollY } from "@/ui/hooks/useWindowScrollY";

export const ProfilePage = () => {
  const { accountId: routeAccountId } = useParams();
  const { session } = useSession();
  const cache = useViewCache();
  const selfAccountId = session.kind === "Authenticated" ? session.account.id : null;
  const accountId = routeAccountId ?? selfAccountId;
  const isSelf = Boolean(accountId && selfAccountId && accountId === selfAccountId);
  const cached = accountId ? cache.getProfile(accountId) : null;

  const [profile, setProfile] = useState<AccountProfile | null>(cached?.profile ?? null);
  const [statuses, setStatuses] = useState<ReadonlyArray<Status>>(cached?.statuses ?? []);
  const [loading, setLoading] = useState(!cached);
  const [loadingMore, setLoadingMore] = useState(false);
  const [editing, setEditing] = useState(false);
  const [error, setError] = useState("");
  const fetchedAtRef = useRef(cached?.fetchedAt ?? 0);
  const profileRef = useRef(profile);
  const statusesRef = useRef(statuses);
  const scrollYRef = useWindowScrollY();
  profileRef.current = profile;
  statusesRef.current = statuses;

  useEffect(() => {
    if (!accountId) {
      return undefined;
    }
    const snapshot = cache.getProfile(accountId);
    if (snapshot) {
      setProfile(snapshot.profile);
      setStatuses(snapshot.statuses);
      fetchedAtRef.current = snapshot.fetchedAt;
      setLoading(false);
      requestAnimationFrame(() => window.scrollTo(0, snapshot.scrollY));
    } else {
      setProfile(null);
      setStatuses([]);
      fetchedAtRef.current = 0;
      setLoading(true);
    }
    setEditing(false);
    setError("");

    let active = true;
    const skipFetch = snapshot !== null && ViewCache.isFresh(snapshot.fetchedAt);
    if (!skipFetch) {
      void Promise.all([
        fetchAccountProfile(accountId),
        fetchAccountStatuses(accountId, { excludeReplies: true }),
      ])
        .then(([profileResult, statusesResult]) => {
          if (!active) {
            return;
          }
          if (profileResult.isErr()) {
            throw new Error(mastodonErrorMessage(profileResult.error));
          }
          if (statusesResult.isErr()) {
            throw new Error(mastodonErrorMessage(statusesResult.error));
          }
          fetchedAtRef.current = Date.now();
          setProfile(profileResult.value);
          setStatuses(statusesResult.value);
          cache.writeProfile(accountId, {
            profile: profileResult.value,
            statuses: statusesResult.value,
            fetchedAt: fetchedAtRef.current,
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
    }

    return () => {
      active = false;
      const currentProfile = profileRef.current ?? snapshot?.profile;
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
  }, [accountId, cache]);

  const handleLoadMore = async () => {
    if (!accountId) {
      return;
    }
    const last = statuses.at(-1);
    if (!last || loadingMore) {
      return;
    }
    setLoadingMore(true);
    const result = await fetchAccountStatuses(accountId, {
      maxId: last.id,
      excludeReplies: true,
    });
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
    } else {
      setStatuses((current) => [...current, ...result.value]);
    }
    setLoadingMore(false);
  };

  const replaceStatus = (updated: Status) => {
    setStatuses((current) => StatusModel.replaceInList(current, updated));
    cache.patchStatus(updated);
  };

  const handleFavourite = async (status: Status) => {
    const result = status.favourited
      ? await unfavouriteStatus(status.id)
      : await favouriteStatus(status.id);
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    replaceStatus(result.value);
  };

  const handleReblog = async (status: Status) => {
    const result = status.reblogged
      ? await unreblogStatus(status.id)
      : await reblogStatus(status.id);
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    replaceStatus(result.value);
  };

  const handleBookmark = async (status: Status) => {
    const result = status.bookmarked
      ? await unbookmarkStatus(status.id)
      : await bookmarkStatus(status.id);
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    replaceStatus(result.value);
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
              <p className="profile-acct">@{profile.acct}</p>
              {isSelf ? (
                <button
                  type="button"
                  className="app-button app-button-secondary profile-edit-button"
                  onClick={() => setEditing(true)}
                >
                  プロフィールを編集
                </button>
              ) : null}
            </div>
            {profile.note ? (
              <div className="profile-note" dangerouslySetInnerHTML={{ __html: profile.note }} />
            ) : null}
            <p className="profile-stats app-muted">
              <span>{profile.statusesCount} 投稿</span>
              <span>{profile.followingCount} フォロー</span>
              <span>{profile.followersCount} フォロワー</span>
            </p>
          </div>
        </header>
      ) : null}
      <div className="timeline">
        {statuses.map((status) => (
          <StatusCard
            key={status.id}
            status={status}
            onFavourite={(body) => void handleFavourite(body)}
            onReblog={(body) => void handleReblog(body)}
            onBookmark={(body) => void handleBookmark(body)}
          />
        ))}
      </div>
      {statuses.length > 0 ? (
        <div className="timeline-footer">
          <button
            type="button"
            className="app-button app-button-secondary"
            onClick={() => void handleLoadMore()}
            disabled={loadingMore}
          >
            {loadingMore ? "読み込み中…" : "もっと見る"}
          </button>
        </div>
      ) : null}
    </AppShell>
  );
};
