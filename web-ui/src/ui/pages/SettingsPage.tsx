import { WebUiPhase } from "@/plan/phases";
import { AppShell } from "@/ui/components/AppShell";

/** TODO(Phase 3): Replace placeholders with preferences forms and logout actions. */
export const SettingsPage = () => (
  <AppShell title="設定">
    <div className="app-card settings-section" data-phase={WebUiPhase.settings}>
      <h2>アカウント</h2>
      <p className="app-muted">TODO(Phase 3): 表示名・プロフィール編集を接続</p>
    </div>
    <div className="app-card settings-section" data-phase={WebUiPhase.settings}>
      <h2>通知</h2>
      <p className="app-muted">TODO(Phase 3): `/api/v1/notifications/policy` を接続</p>
    </div>
    <div className="app-card settings-section" data-phase={WebUiPhase.settings}>
      <h2>フィルター</h2>
      <p className="app-muted">TODO(Phase 3): ミュート・ブロック一覧を接続</p>
    </div>
    <div className="app-card settings-section" data-phase={WebUiPhase.settings}>
      <h2>セッション</h2>
      <p className="app-muted">TODO(Phase 3): `/app/logout` 導線を整理</p>
    </div>
  </AppShell>
);
