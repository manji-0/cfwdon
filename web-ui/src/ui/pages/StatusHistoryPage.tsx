import { useEffect, useState } from "react";
import { Link, useParams } from "react-router";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { StatusEdit } from "@/domain/status/edit";
import { fetchStatusHistory } from "@/infrastructure/api/status";
import { AppShell } from "@/ui/components/AppShell";
import { StatusContent } from "@/ui/components/StatusContent";
import { formatRelativeTime } from "@/ui/lib/time";

export const StatusHistoryPage = () => {
  const { statusId = "" } = useParams();
  const [edits, setEdits] = useState<ReadonlyArray<StatusEdit>>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!statusId) {
      return;
    }
    let active = true;
    setLoading(true);
    void fetchStatusHistory(statusId).then((result) => {
      if (!active) {
        return;
      }
      setLoading(false);
      if (result.isErr()) {
        setError(mastodonErrorMessage(result.error));
        return;
      }
      setEdits(result.value);
    });
    return () => {
      active = false;
    };
  }, [statusId]);

  return (
    <AppShell title="編集履歴">
      <p className="thread-back">
        <Link to={`/status/${statusId}`}>← 投稿に戻る</Link>
      </p>
      {error ? <p className="app-error">{error}</p> : null}
      {loading ? <div className="app-status">読み込み中…</div> : null}
      <div className="timeline">
        {edits.map((edit) => (
          <article key={`${edit.createdAt}-${edit.content.slice(0, 24)}`} className="status-card">
            <p className="app-muted">{formatRelativeTime(edit.createdAt)}</p>
            {edit.spoilerText ? <p className="app-muted">CW: {edit.spoilerText}</p> : null}
            <StatusContent html={edit.content} />
          </article>
        ))}
      </div>
      {!loading && edits.length === 0 ? (
        <div className="app-card">
          <p className="app-muted">編集履歴はありません。</p>
        </div>
      ) : null}
    </AppShell>
  );
};
