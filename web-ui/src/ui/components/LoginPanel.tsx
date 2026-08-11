import type { SessionState } from "@/domain/session/session";

type LoginPanelProps = Readonly<{
  session: SessionState;
}>;

export const LoginPanel = ({ session }: LoginPanelProps) => {
  if (session.kind === "Loading") {
    return <div className="app-status">読み込み中…</div>;
  }
  if (session.kind === "Failed") {
    return <div className="app-status app-error">{session.message}</div>;
  }
  if (session.kind === "Authenticated") {
    return null;
  }

  return (
    <section className="app-login-panel">
      <h1>cfwdon へようこそ</h1>
      <p className="app-muted">ログインしてタイムラインを見たり投稿したりできます。</p>
      <a className="app-button" href="/app/login">
        ログイン
      </a>
    </section>
  );
};
