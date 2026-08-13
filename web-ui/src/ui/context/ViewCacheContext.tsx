import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  ViewCache,
  type NotificationsSnapshot,
  type ProfileSnapshot,
  type TimelineSnapshot,
  type ViewCacheState,
} from "@/domain/cache/view-cache";
import type { Status } from "@/domain/status/status";
import { StreamingUser } from "@/infrastructure/streaming/mastodon-stream";

type ViewCacheContextValue = Readonly<{
  getHome: () => TimelineSnapshot | null;
  getNotifications: () => NotificationsSnapshot | null;
  getProfile: (accountId: string) => ProfileSnapshot | null;
  writeHome: (snapshot: TimelineSnapshot) => void;
  writeNotifications: (snapshot: NotificationsSnapshot) => void;
  writeProfile: (accountId: string, snapshot: ProfileSnapshot) => void;
  patchStatus: (updated: Status) => void;
}>;

const ViewCacheContext = createContext<ViewCacheContextValue | null>(null);

export const ViewCacheProvider = ({ children }: Readonly<{ children: ReactNode }>) => {
  const [state, setState] = useState<ViewCacheState>(ViewCache.empty);
  const stateRef = useRef(state);
  stateRef.current = state;

  const getHome = useCallback(() => stateRef.current.home, []);
  const getNotifications = useCallback(() => stateRef.current.notifications, []);
  const getProfile = useCallback(
    (accountId: string) => stateRef.current.profiles.get(accountId) ?? null,
    [],
  );

  const writeHome = useCallback((snapshot: TimelineSnapshot) => {
    setState((current) => ViewCache.writeHome(current, snapshot));
  }, []);

  const writeNotifications = useCallback((snapshot: NotificationsSnapshot) => {
    setState((current) => ViewCache.writeNotifications(current, snapshot));
  }, []);

  const writeProfile = useCallback((accountId: string, snapshot: ProfileSnapshot) => {
    setState((current) => ViewCache.writeProfile(current, accountId, snapshot));
  }, []);

  const patchStatus = useCallback((updated: Status) => {
    setState((current) => ViewCache.patchStatus(current, updated));
  }, []);

  useEffect(() => {
    const subscription = StreamingUser.subscribe((event) => {
      if (event.kind === "conversation") {
        return;
      }
      setState((current) => ViewCache.applyStreamEvent(current, event));
    });
    return () => subscription.close();
  }, []);

  const value = useMemo(
    (): ViewCacheContextValue => ({
      getHome,
      getNotifications,
      getProfile,
      writeHome,
      writeNotifications,
      writeProfile,
      patchStatus,
    }),
    [getHome, getNotifications, getProfile, writeHome, writeNotifications, writeProfile, patchStatus],
  );

  return <ViewCacheContext.Provider value={value}>{children}</ViewCacheContext.Provider>;
};

export const useViewCache = (): ViewCacheContextValue => {
  const value = useContext(ViewCacheContext);
  if (!value) {
    throw new Error("useViewCache must be used within ViewCacheProvider");
  }
  return value;
};
