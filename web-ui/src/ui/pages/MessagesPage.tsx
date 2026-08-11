import { WebUiPhase } from "@/plan/phases";
import { AppShell } from "@/ui/components/AppShell";

/** TODO(Phase 5): Direct message conversations via `/api/v1/conversations`. */
export const MessagesPage = () => (
  <AppShell title="メッセージ">
    <div className="app-card" data-phase={WebUiPhase.collections}>
      <p className="app-muted">TODO(Phase 5): DM 会話一覧と送信 UI を接続</p>
    </div>
  </AppShell>
);
