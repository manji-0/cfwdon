import type { ReactNode } from "react";

type ListStatusProps = Readonly<{
  loading: boolean;
  error: string;
  isEmpty: boolean;
  empty: ReactNode;
  onRetry?: () => void;
}>;

/**
 * Error / loading / empty feedback for a paged list, rendered above the items.
 * The empty message is suppressed while loading or after an error so a failed
 * load never reads as "nothing here yet".
 */
export const ListStatus = ({ loading, error, isEmpty, empty, onRetry }: ListStatusProps) => (
  <>
    {error ? (
      <div className="app-error list-status-error" role="alert">
        <span>{error}</span>
        {onRetry && !loading ? (
          <button type="button" className="app-button app-button-secondary" onClick={onRetry}>
            再試行
          </button>
        ) : null}
      </div>
    ) : null}
    {loading ? <div className="app-status">読み込み中…</div> : null}
    {!loading && !error && isEmpty ? (
      <div className="app-card">
        <p className="app-muted">{empty}</p>
      </div>
    ) : null}
  </>
);
