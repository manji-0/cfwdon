import { AppShell } from "@/ui/components/AppShell";

export const HomePage = () => (
  <AppShell
    title="ホーム"
    aside={
      <>
        <div className="app-card">
          <h2>トレンド</h2>
          <p className="app-muted">Phase 1 でトレンドを表示します。</p>
        </div>
        <div className="app-card">
          <h2>おすすめ</h2>
          <p className="app-muted">フォロー候補はここに表示されます。</p>
        </div>
      </>
    }
  >
    <section className="app-composer" aria-label="新規投稿">
      <textarea placeholder="いまどうしてる？" disabled />
      <button type="button" className="app-button" disabled>
        投稿
      </button>
    </section>
    <div className="app-card">
      <p className="app-muted">ホームタイムラインは Phase 1 で接続します。</p>
    </div>
  </AppShell>
);
