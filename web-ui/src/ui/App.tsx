import { useEffect, useState } from "react";
import { BrowserRouter, Navigate, Route, Routes } from "react-router-dom";
import { loadSession } from "@/application/load-session";
import type { SessionState } from "@/domain/session/session";
import { SessionState as Session } from "@/domain/session/session";
import { LoginPanel } from "@/ui/components/LoginPanel";
import { SessionProvider } from "@/ui/context/SessionContext";
import { HomePage } from "@/ui/pages/HomePage";
import { PlaceholderPage } from "@/ui/pages/PlaceholderPage";
import { ProfilePage } from "@/ui/pages/ProfilePage";
import { ThreadPage } from "@/ui/pages/ThreadPage";
import "@/ui/styles/app.css";

const AppRoutes = ({ session }: Readonly<{ session: SessionState }>) => {
  if (session.kind === "Anonymous" || session.kind === "Failed") {
    return <LoginPanel session={session} />;
  }

  if (session.kind === "Loading") {
    return <div className="app-status">読み込み中…</div>;
  }

  return (
    <SessionProvider value={{ session }}>
      <Routes>
        <Route path="/" element={<HomePage />} />
        <Route path="/status/:statusId" element={<ThreadPage />} />
        <Route path="/profile" element={<ProfilePage />} />
        <Route path="/profile/:accountId" element={<ProfilePage />} />
        <Route
          path="/notifications"
          element={<PlaceholderPage title="通知" message="通知は Phase 2 で接続します。" />}
        />
        <Route
          path="/search"
          element={<PlaceholderPage title="検索" message="検索は Phase 2 で接続します。" />}
        />
        <Route
          path="/settings"
          element={<PlaceholderPage title="設定" message="設定は Phase 3 で接続します。" />}
        />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </SessionProvider>
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
