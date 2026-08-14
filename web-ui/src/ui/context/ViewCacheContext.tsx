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
import type { CachedView as CachedSlot } from "@/domain/cache/cached-view";
import { ProfileSet } from "@/domain/cache/profile-set";
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
  getHome: () => CachedSlot<TimelineSnapshot>;
  getNotifications: () => CachedSlot<NotificationsSnapshot>;
  getProfile: (accountId: string) => CachedSlot<ProfileSnapshot>;
  writeHome: (snapshot: TimelineSnapshot) => void;
  writeNotifications: (snapshot: NotificationsSnapshot) => void;
  writeProfile: (accountId: string, snapshot: ProfileSnapshot) => void;
  receivePreloadedProfile: (accountId: string, snapshot: ProfileSnapshot) => void;
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
    (accountId: string) => ProfileSet.lookup(stateRef.current.profiles, accountId),
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

  const receivePreloadedProfile = useCallback((accountId: string, snapshot: ProfileSnapshot) => {
    setState((current) => ViewCache.receivePreloadedProfile(current, accountId, snapshot));
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
      receivePreloadedProfile,
      patchStatus,
    }),
    [
      getHome,
      getNotifications,
      getProfile,
      writeHome,
      writeNotifications,
      writeProfile,
      receivePreloadedProfile,
      patchStatus,
    ],
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
