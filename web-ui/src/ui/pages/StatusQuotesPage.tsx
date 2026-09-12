import { useCallback } from "react";
import { Link, useParams } from "react-router";
import { AppRoute } from "@/domain/navigation/route";
import { fetchStatusQuotes } from "@/infrastructure/api/status";
import { StatusCollectionPage } from "@/ui/pages/StatusCollectionPage";

export const StatusQuotesPage = () => {
  const { statusId } = useParams();
  const fetchPage = useCallback(
    (query: { maxId?: string; limit?: number }) => fetchStatusQuotes(statusId ?? "", query),
    [statusId],
  );

  return (
    <StatusCollectionPage
      title="引用"
      emptyMessage="この投稿への引用はまだありません。"
      header={
        <p className="thread-back">
          <Link to={statusId ? AppRoute.toPath(AppRoute.status(statusId)) : AppRoute.toPath(AppRoute.home())}>
            ← 投稿に戻る
          </Link>
        </p>
      }
      fetchPage={fetchPage}
    />
  );
};
