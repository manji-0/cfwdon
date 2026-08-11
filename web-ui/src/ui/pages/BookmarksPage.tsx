import { WebUiPhase } from "@/plan/phases";
import { AppShell } from "@/ui/components/AppShell";

/** TODO(Phase 5): Replace with bookmark timeline using `fetchBookmarks`. */
export const BookmarksPage = () => (
  <AppShell title="ブックマーク">
    <div className="app-card" data-phase={WebUiPhase.collections}>
      <p className="app-muted">TODO(Phase 5): ブックマーク一覧を接続</p>
    </div>
  </AppShell>
);
