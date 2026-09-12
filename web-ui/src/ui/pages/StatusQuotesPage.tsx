import { useCallback } from "react";
import { useAppParams } from "@/ui/hooks/useAppParams";
import { AppLink } from "@/ui/lib/app-link";
import { AppRoute } from "@/domain/navigation/route";
import { fetchStatusQuotes } from "@/infrastructure/api/status";
import { StatusCollectionPage } from "@/ui/pages/StatusCollectionPage";

export const StatusQuotesPage = () => {
  const { statusId } = useAppParams();
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
          <AppLink to={statusId ? AppRoute.status(statusId) : AppRoute.home()}>
            ← 投稿に戻る
          </AppLink>
        </p>
      }
      fetchPage={fetchPage}
    />
  );
};
