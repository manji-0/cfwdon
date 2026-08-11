import { useEffect, useState } from "react";
import { BrowserRouter, Navigate, Route, Routes } from "react-router-dom";
import { loadSession } from "@/application/load-session";
import type { SessionState } from "@/domain/session/session";
import { SessionState as Session } from "@/domain/session/session";
import { LoginPanel } from "@/ui/components/LoginPanel";
import { HomePage } from "@/ui/pages/HomePage";
import { PlaceholderPage } from "@/ui/pages/PlaceholderPage";
import "@/ui/styles/app.css";

const AppRoutes = ({ session }: Readonly<{ session: SessionState }>) => {
  if (session.kind === "Anonymous" || session.kind === "Failed") {
    return <LoginPanel session={session} />;
  }

  if (session.kind === "Loading") {
    return <div className="app-status">読み込み中…</div>;
  }

  return (
    <Routes>
      <Route path="/" element={<HomePage />} />
      <Route
        path="/notifications"
        element={<PlaceholderPage title="通知" message="通知は Phase 2 で接続します。" />}
      />
      <Route
        path="/search"
        element={<PlaceholderPage title="検索" message="検索は Phase 2 で接続します。" />}
      />
      <Route
        path="/profile"
        element={
          <PlaceholderPage
            title="プロフィール"
            message={`@${session.account.username} のプロフィールは Phase 1 で接続します。`}
          />
        }
      />
      <Route
        path="/settings"
        element={<PlaceholderPage title="設定" message="設定は Phase 3 で接続します。" />}
      />
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
};

export const App = () => {
  const [session, setSession] = useState<SessionState>(Session.loading());

  useEffect(() => {
    let active = true;
    void loadSession().then((result) => {
      if (active && result.isOk()) {
        setSession(result.value);
      }
    });
    return () => {
      active = false;
    };
  }, []);

  return (
    <BrowserRouter basename="/app">
      <AppRoutes session={session} />
    </BrowserRouter>
  );
};
