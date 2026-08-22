import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import { fetchUnreadNotificationCount } from "@/infrastructure/api/notification";
import { StreamingUser } from "@/infrastructure/streaming/mastodon-stream";

type UnreadNotificationsContextValue = Readonly<{
  unreadCount: number;
  refreshUnreadCount: () => void;
  clearUnreadCount: () => void;
}>;

const UnreadNotificationsContext = createContext<UnreadNotificationsContextValue | null>(null);

export const UnreadNotificationsProvider = ({ children }: Readonly<{ children: ReactNode }>) => {
  const [unreadCount, setUnreadCount] = useState(0);

  const refreshUnreadCount = useCallback(() => {
    void fetchUnreadNotificationCount().then((result) => {
      if (result.isOk()) {
        setUnreadCount(result.value);
      }
    });
  }, []);

  useEffect(() => {
    refreshUnreadCount();
  }, [refreshUnreadCount]);

  useEffect(() => {
    const subscription = StreamingUser.subscribe((event) => {
      if (event.kind === "Notification") {
        refreshUnreadCount();
      }
    });
    return () => subscription.close();
  }, [refreshUnreadCount]);

  const value = useMemo(
    () => ({
      unreadCount,
      refreshUnreadCount,
      clearUnreadCount: () => setUnreadCount(0),
    }),
    [unreadCount, refreshUnreadCount],
  );

  return (
    <UnreadNotificationsContext.Provider value={value}>
      {children}
    </UnreadNotificationsContext.Provider>
  );
};

export const useUnreadNotifications = (): UnreadNotificationsContextValue => {
  const value = useContext(UnreadNotificationsContext);
  if (!value) {
    throw new Error("useUnreadNotifications must be used within UnreadNotificationsProvider");
  }
  return value;
};
