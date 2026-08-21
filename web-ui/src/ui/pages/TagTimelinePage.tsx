import { useCallback } from "react";
import { useParams } from "react-router-dom";
import { fetchTagTimeline } from "@/infrastructure/api/status";
import { SearchSidebar } from "@/ui/components/SearchSidebar";
import { TrendsSidebar } from "@/ui/components/TrendsSidebar";
import { StatusCollectionPage } from "@/ui/pages/StatusCollectionPage";

export const TagTimelinePage = () => {
  const { tagName = "" } = useParams();
  const tag = decodeURIComponent(tagName).replace(/^#/, "");
  const fetchPage = useCallback(
    (query: { maxId?: string; limit?: number }) => fetchTagTimeline(tag, query),
    [tag],
  );

  return (
    <StatusCollectionPage
      title={`#${tag}`}
      emptyMessage={`#${tag} の投稿はまだありません。`}
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
