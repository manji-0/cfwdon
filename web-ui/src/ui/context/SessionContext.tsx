import { createContext, useContext } from "react";
import type { SessionState } from "@/domain/session/session";

export type SessionContextValue = Readonly<{
  session: SessionState;
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
