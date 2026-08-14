import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import { ConversationSet } from "@/domain/conversations/conversation-set";
import { fetchConversations } from "@/infrastructure/api/conversations";
import { StreamingUser } from "@/infrastructure/streaming/mastodon-stream";

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
        setUnreadCount(ConversationSet.unreadCount(result.value));
      }
    });
  }, []);

  useEffect(() => {
    refreshUnreadCount();
  }, [refreshUnreadCount]);

  useEffect(() => {
    const subscription = StreamingUser.subscribe((event) => {
      if (event.kind === "conversation") {
        refreshUnreadCount();
        return;
      }
      if (event.kind === "notification") {
        refreshUnreadCount();
        return;
      }
      if (event.kind === "update" && event.status.visibility.kind === "Direct") {
        refreshUnreadCount();
      }
    });
    return () => subscription.close();
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
