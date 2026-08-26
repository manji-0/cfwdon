import { useCallback } from "react";
import { fetchFavourites } from "@/infrastructure/api/favourites";
import { StatusCollectionPage } from "@/ui/pages/StatusCollectionPage";

export const FavouritesPage = () => {
  const fetchPage = useCallback(
    (query: { maxId?: string; limit?: number }) => fetchFavourites(query),
    [],
  );

  return (
    <StatusCollectionPage
      title="お気に入り"
      emptyMessage="いいねした投稿はまだありません。"
      fetchPage={fetchPage}
    />
  );
};
