import { useEffect, useState } from "react";
import { Link } from "react-router";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { TrendTag } from "@/domain/trends/trend";
import { fetchTrendingTags } from "@/infrastructure/api/trends";
import { WebUiPhase } from "@/plan/phases";

const trendUsesLabel = (tag: TrendTag): string => {
  const latest = tag.history.at(0);
  if (!latest) {
    return "";
  }
  const uses = Number.parseInt(latest.uses, 10);
  if (Number.isNaN(uses)) {
    return "";
  }
  return `${uses.toLocaleString()} 件の投稿`;
};

export const TrendsSidebar = () => {
  const [tags, setTags] = useState<ReadonlyArray<TrendTag>>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  useEffect(() => {
    let active = true;
    void (async () => {
      const result = await fetchTrendingTags({ limit: 10 });
      if (!active) {
        return;
      }
      if (result.isErr()) {
        setError(mastodonErrorMessage(result.error));
      } else {
        setTags(result.value);
      }
      setLoading(false);
    })();
    return () => {
      active = false;
    };
  }, []);

  return (
    <section className="app-card trends-sidebar" data-phase={WebUiPhase.timelineMedia}>
      <h2>トレンド</h2>
      {loading ? <p className="app-muted">読み込み中…</p> : null}
      {error ? <p className="app-error">{error}</p> : null}
      {!loading && !error && tags.length === 0 ? (
        <p className="app-muted">トレンドはまだありません</p>
      ) : null}
      {!loading && tags.length > 0 ? (
        <ol className="trends-list">
          {tags.map((tag) => (
            <li key={tag.id}>
              <Link className="trends-tag" to={`/tags/${encodeURIComponent(tag.name)}`}>
                <span className="trends-tag-name">#{tag.name}</span>
                {trendUsesLabel(tag) ? (
                  <span className="app-muted trends-tag-uses">{trendUsesLabel(tag)}</span>
                ) : null}
              </Link>
            </li>
          ))}
        </ol>
      ) : null}
    </section>
  );
};
