import { useCallback } from "react";
import { useLocation } from "react-router";
import { fetchPublicTimeline } from "@/infrastructure/api/status";
import { StatusCollectionPage } from "@/ui/pages/StatusCollectionPage";

export const PublicTimelinePage = () => {
  const { pathname } = useLocation();
  const local = pathname.endsWith("/local");
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
