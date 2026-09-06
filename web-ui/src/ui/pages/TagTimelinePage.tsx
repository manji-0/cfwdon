import { useCallback, useEffect, useState } from "react";
import { useParams } from "react-router";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import { fetchTagTimeline } from "@/infrastructure/api/status";
import { fetchFollowedTags, followTag, unfollowTag } from "@/infrastructure/api/tags";
import { SearchSidebar } from "@/ui/components/SearchSidebar";
import { TrendsSidebar } from "@/ui/components/TrendsSidebar";
import { StatusCollectionPage } from "@/ui/pages/StatusCollectionPage";

export const TagTimelinePage = () => {
  const { tagName = "" } = useParams();
  const tag = decodeURIComponent(tagName).replace(/^#/, "");
  const [following, setFollowing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [followError, setFollowError] = useState("");
  const fetchPage = useCallback(
    (query: { maxId?: string; limit?: number }) => fetchTagTimeline(tag, query),
    [tag],
  );

  useEffect(() => {
    let active = true;
    setFollowError("");
    void fetchFollowedTags().then((result) => {
      if (!active || result.isErr()) {
        return;
      }
      setFollowing(result.value.some((item) => item.name.toLowerCase() === tag.toLowerCase()));
    });
    return () => {
      active = false;
    };
  }, [tag]);

  const handleFollowToggle = async () => {
    setSaving(true);
    setFollowError("");
    const result = following ? await unfollowTag(tag) : await followTag(tag);
    setSaving(false);
    if (result.isErr()) {
      setFollowError(mastodonErrorMessage(result.error));
      return;
    }
    setFollowing(result.value.following);
  };

  return (
    <StatusCollectionPage
      title={`#${tag}`}
      emptyMessage={`#${tag} の投稿はまだありません。`}
      header={
        <div className="tag-follow-header">
          <button
            type="button"
            className="app-button"
            disabled={saving}
            onClick={() => void handleFollowToggle()}
          >
            {following ? "フォロー解除" : "ハッシュタグをフォロー"}
          </button>
          {followError ? <p className="app-error">{followError}</p> : null}
        </div>
      }
      aside={
        <>
          <SearchSidebar />
          <TrendsSidebar />
        </>
      }
      fetchPage={fetchPage}
    />
  );
};
