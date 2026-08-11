import { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { AccountProfile } from "@/domain/account/account";
import type { Status } from "@/domain/status/status";
import { Status as StatusModel } from "@/domain/status/status";
import {
  favouriteStatus,
  reblogStatus,
  unfavouriteStatus,
  unreblogStatus,
} from "@/infrastructure/api/status";
import { fetchAccountProfile, fetchAccountStatuses } from "@/infrastructure/api/account";
import { AppShell } from "@/ui/components/AppShell";
import { StatusCard } from "@/ui/components/StatusCard";
import { useSession } from "@/ui/context/SessionContext";

export const ProfilePage = () => {
  const { accountId: routeAccountId } = useParams();
  const { session } = useSession();
  const selfAccountId =
    session.kind === "Authenticated" ? session.account.id : null;
  const accountId = routeAccountId ?? selfAccountId;

  const [profile, setProfile] = useState<AccountProfile | null>(null);
  const [statuses, setStatuses] = useState<ReadonlyArray<Status>>([]);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!accountId) {
      return;
    }
    let active = true;
    setLoading(true);
    setError("");
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
        setProfile(profileResult.value);
        setStatuses(statusesResult.value);
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
    return () => {
      active = false;
    };
  }, [accountId]);

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
    setStatuses((current) =>
      current.map((status) => {
        const body = StatusModel.displayBody(status);
        if (body.id === updated.id) {
          return status.reblog ? { ...status, reblog: updated } : updated;
        }
        return status;
      }),
    );
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
      {profile ? (
        <header className="profile-header">
          <div
            className="profile-header-banner"
            style={{ backgroundImage: profile.header ? `url(${profile.header})` : undefined }}
          />
          <div className="profile-header-body">
            <img className="profile-avatar" src={profile.avatar} alt="" />
            <div>
              <h1 className="profile-name">{profile.displayName || profile.username}</h1>
              <p className="profile-acct">@{profile.acct}</p>
            </div>
            {profile.note ? (
              <div
                className="profile-note"
                dangerouslySetInnerHTML={{ __html: profile.note }}
              />
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
