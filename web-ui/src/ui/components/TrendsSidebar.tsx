import { WebUiPhase } from "@/plan/phases";

/** TODO(Phase 1): Render `fetchTrendingTags` results and link into search. */
export const TrendsSidebar = () => (
  <section className="app-card trends-sidebar" data-phase={WebUiPhase.timelineMedia}>
    <h2>トレンド</h2>
    <p className="app-muted">Phase 1 でトレンドタグを表示します。</p>
  </section>
);
