import { useCallback } from "react";
import { useLocation } from "react-router-dom";
import { SearchSidebar } from "@/ui/components/SearchSidebar";
import { TimelineTabs } from "@/ui/components/TimelineTabs";
import { TrendsSidebar } from "@/ui/components/TrendsSidebar";
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
      header={<TimelineTabs />}
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
