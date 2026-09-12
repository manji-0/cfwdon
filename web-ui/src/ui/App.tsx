import { useEffect, useState } from "react";
import { RouterProvider } from "@tanstack/react-router";
import { loadSession } from "@/application/load-session";
import { SessionState } from "@/domain/session/session";
import { LoginPanel } from "@/ui/components/LoginPanel";
import { SessionProvider, createSessionContextValue } from "@/ui/context/SessionContext";
import { appRouter } from "@/ui/router";
import "@/ui/styles/app.css";

const RouteFallback = () => <div className="app-status">読み込み中…</div>;

export const App = () => {
  const [session, setSession] = useState<SessionState>(SessionState.loading());

  useEffect(() => {
    let active = true;
    void loadSession().then((result) => {
      if (active && result.isOk()) {
        setSession((current) =>
          current.kind === "Loading" ? SessionState.resolve(current, result.value) : current,
        );
      }
    });
    return () => {
      active = false;
    };
  }, []);

  if (session.kind === "Anonymous" || session.kind === "Failed") {
    return <LoginPanel session={session} />;
  }

  if (session.kind === "Loading") {
    return <RouteFallback />;
  }

  return (
    <SessionProvider value={createSessionContextValue(session, setSession)}>
      <RouterProvider router={appRouter} />
    </SessionProvider>
  );
};
