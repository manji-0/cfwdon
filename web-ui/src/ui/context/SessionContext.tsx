import { createContext, useContext } from "react";
import type { SessionState } from "@/domain/session/session";
import { SessionState as Session } from "@/domain/session/session";

export type SessionContextValue = Readonly<{
  session: SessionState;
  setSession: (session: SessionState) => void;
  clearSession: () => void;
}>;

const SessionContext = createContext<SessionContextValue | null>(null);

export const SessionProvider = SessionContext.Provider;

export const useSession = (): SessionContextValue => {
  const value = useContext(SessionContext);
  if (!value) {
    throw new Error("useSession must be used within SessionProvider");
  }
  return value;
};

export const createSessionContextValue = (
  session: SessionState,
  setSession: (session: SessionState) => void,
): SessionContextValue => ({
  session,
  setSession,
  clearSession: () => setSession(Session.anonymous()),
});
