import { Suspense, useEffect, useState } from "react";
import { BrowserRouter, Navigate, Route, Routes } from "react-router-dom";
import { loadSession } from "@/application/load-session";
import type { SessionState } from "@/domain/session/session";
import { SessionState as Session } from "@/domain/session/session";
import { KeyboardShortcutsHelp } from "@/ui/components/KeyboardShortcutsHelp";
import { LoginPanel } from "@/ui/components/LoginPanel";
import { SessionProvider, createSessionContextValue } from "@/ui/context/SessionContext";
import { HomePage } from "@/ui/pages/HomePage";
import {
  NotificationsPage,
  ProfilePage,
  SearchPage,
  SettingsPage,
  ThreadPage,
} from "@/ui/pages/lazy-pages";
import "@/ui/styles/app.css";

const RouteFallback = () => <div className="app-status">読み込み中…</div>;

// TODO(Phase 5): Register BookmarksPage, ListsPage, and MessagesPage routes.
const AppRoutes = ({
  session,
  setSession,
}: Readonly<{ session: SessionState; setSession: (session: SessionState) => void }>) => {
  if (session.kind === "Anonymous" || session.kind === "Failed") {
    return <LoginPanel session={session} />;
  }

  if (session.kind === "Loading") {
    return <RouteFallback />;
  }

  return (
    <SessionProvider value={createSessionContextValue(session, setSession)}>
      <KeyboardShortcutsHelp />
      <Suspense fallback={<RouteFallback />}>
        <Routes>
          <Route path="/" element={<HomePage />} />
          <Route path="/status/:statusId" element={<ThreadPage />} />
          <Route path="/profile" element={<ProfilePage />} />
          <Route path="/profile/:accountId" element={<ProfilePage />} />
          <Route path="/notifications" element={<NotificationsPage />} />
          <Route path="/search" element={<SearchPage />} />
          <Route path="/settings" element={<SettingsPage />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </Suspense>
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
      <AppRoutes session={session} setSession={setSession} />
    </BrowserRouter>
  );
};
