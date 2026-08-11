import { WebUiPhase } from "@/plan/phases";
import { AppShell } from "@/ui/components/AppShell";

/** TODO(Phase 5): Lists CRUD and list timelines via `/api/v1/lists`. */
export const ListsPage = () => (
  <AppShell title="リスト">
    <div className="app-card" data-phase={WebUiPhase.collections}>
      <p className="app-muted">TODO(Phase 5): リスト管理とリスト TL を接続</p>
    </div>
  </AppShell>
);
