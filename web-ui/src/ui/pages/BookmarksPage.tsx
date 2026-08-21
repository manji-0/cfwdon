import { useCallback } from "react";
import { fetchBookmarks } from "@/infrastructure/api/bookmarks";
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
      fetchPage={fetchPage}
    />
  );
};
