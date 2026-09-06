import { useInfiniteScroll } from "@/ui/hooks/useInfiniteScroll";

type LoadMoreFooterProps = Readonly<{
  hasMore: boolean;
  loading: boolean;
  onLoadMore: () => void;
  observeKey?: string | number;
}>;

export const LoadMoreFooter = ({ hasMore, loading, onLoadMore, observeKey }: LoadMoreFooterProps) => {
  const sentinelRef = useInfiniteScroll<HTMLDivElement>({
    enabled: hasMore && !loading,
    onLoadMore,
    observeKey,
  });

  if (!hasMore) {
    return null;
  }

  return (
    <div className="timeline-footer">
      <div ref={sentinelRef} className="timeline-scroll-sentinel" aria-hidden="true" />
      <button
        type="button"
        className="app-button app-button-secondary"
        onClick={onLoadMore}
        disabled={loading}
        aria-busy={loading}
      >
        {loading ? "読み込み中…" : "もっと見る"}
      </button>
    </div>
  );
};
