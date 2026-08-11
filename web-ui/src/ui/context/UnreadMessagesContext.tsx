import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import { countUnreadConversations } from "@/domain/conversations/unread";
import { fetchConversations } from "@/infrastructure/api/conversations";

type UnreadMessagesContextValue = Readonly<{
  unreadCount: number;
  refreshUnreadCount: () => void;
  setUnreadCount: (count: number) => void;
}>;

const UnreadMessagesContext = createContext<UnreadMessagesContextValue | null>(null);

export const UnreadMessagesProvider = ({ children }: Readonly<{ children: ReactNode }>) => {
  const [unreadCount, setUnreadCount] = useState(0);

  const refreshUnreadCount = useCallback(() => {
    void fetchConversations({ limit: 40 }).then((result) => {
      if (result.isOk()) {
        setUnreadCount(countUnreadConversations(result.value));
      }
    });
  }, []);

  useEffect(() => {
    refreshUnreadCount();
  }, [refreshUnreadCount]);

  const value = useMemo(
    () => ({
      unreadCount,
      refreshUnreadCount,
      setUnreadCount,
    }),
    [unreadCount, refreshUnreadCount],
  );

  return (
    <UnreadMessagesContext.Provider value={value}>{children}</UnreadMessagesContext.Provider>
  );
};

export const useUnreadMessages = (): UnreadMessagesContextValue => {
  const value = useContext(UnreadMessagesContext);
  if (!value) {
    throw new Error("useUnreadMessages must be used within UnreadMessagesProvider");
  }
  return value;
};
