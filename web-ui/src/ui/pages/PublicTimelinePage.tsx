import { useCallback } from "react";
import { useLocation } from "react-router";
import { AppRoute } from "@/domain/navigation/route";
import { fetchPublicTimeline } from "@/infrastructure/api/status";
import { StatusCollectionPage } from "@/ui/pages/StatusCollectionPage";

export const PublicTimelinePage = () => {
  const { pathname } = useLocation();
  const local = AppRoute.isLocalPublicTimeline(pathname);
  const fetchPage = useCallback(
    (query: { maxId?: string; limit?: number }) => fetchPublicTimeline({ ...query, local }),
    [local],
  );

  return (
    <StatusCollectionPage
      title={local ? "ローカル" : "連合"}
      emptyMessage={local ? "ローカルの投稿はまだありません。" : "連合タイムラインはまだ空です。"}
      fetchPage={fetchPage}
    />
  );
};
