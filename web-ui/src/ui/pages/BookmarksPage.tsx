import { useCallback } from "react";
import { fetchBookmarks } from "@/infrastructure/api/bookmarks";
import { MeBackLink } from "@/ui/components/MeHubNav";
import { StatusCollectionPage } from "@/ui/pages/StatusCollectionPage";

export const BookmarksPage = () => {
  const fetchPage = useCallback(
    (query: { maxId?: string; limit?: number }) => fetchBookmarks(query),
    [],
  );

  return (
    <StatusCollectionPage
      title="ブックマーク"
      emptyMessage="ブックマークはまだありません。"
      header={<MeBackLink />}
      fetchPage={fetchPage}
    />
  );
};
