import type { ReactElement, ReactNode } from "react";
import { MemoryRouter } from "react-router";
import { cleanup, render, type RenderResult } from "@testing-library/react";
import { SessionState } from "@/domain/session/session";
import { ViewCache } from "@/domain/cache/view-cache";
import { ProfileSet } from "@/domain/cache/profile-set";
import { ConfirmProvider } from "@/ui/context/ConfirmContext";
import { SessionProvider, createSessionContextValue } from "@/ui/context/SessionContext";
import {
  UnreadMessagesContextProvider,
  type UnreadMessagesContextValue,
} from "@/ui/context/UnreadMessagesContext";
import {
  UnreadNotificationsContextProvider,
  type UnreadNotificationsContextValue,
} from "@/ui/context/UnreadNotificationsContext";
import {
  ViewCacheContextProvider,
  type ViewCacheContextValue,
} from "@/ui/context/ViewCacheContext";
import { resetAnnouncementBannerCache } from "@/ui/components/AnnouncementBanner";
import type { AccountSummary } from "@/domain/session/account";

export const TEST_ACCOUNT = {
  id: "acct-1",
  username: "alice",
  displayName: "Alice",
  acct: "alice",
  avatar: "https://example.test/a.png",
  instanceName: "example.test",
} as const satisfies AccountSummary;

const unreadMessages: UnreadMessagesContextValue = {
  unreadCount: 0,
  refreshUnreadCount: () => undefined,
  setUnreadCount: () => undefined,
};

const unreadNotifications: UnreadNotificationsContextValue = {
  unreadCount: 0,
  refreshUnreadCount: () => undefined,
  clearUnreadCount: () => undefined,
};

export const createMemoryViewCache = (): ViewCacheContextValue => {
  let state = ViewCache.empty();
  return {
    getHome: () => state.home,
    getNotifications: () => state.notifications,
    getProfile: (accountId) => ProfileSet.lookup(state.profiles, accountId),
    writeHome: (snapshot) => {
      state = ViewCache.writeHome(state, snapshot);
    },
    writeNotifications: (snapshot) => {
      state = ViewCache.writeNotifications(state, snapshot);
    },
    writeProfile: (accountId, snapshot) => {
      state = ViewCache.writeProfile(state, accountId, snapshot);
    },
    receivePreloadedProfile: (accountId, snapshot) => {
      state = ViewCache.receivePreloadedProfile(state, accountId, snapshot);
    },
    patchStatus: (updated) => {
      state = ViewCache.patchStatus(state, updated);
    },
  };
};

const AppTestProviders = ({ children }: Readonly<{ children: ReactNode }>) => {
  const session = SessionState.authenticated(TEST_ACCOUNT);
  return (
    <SessionProvider value={createSessionContextValue(session, () => undefined)}>
      <ViewCacheContextProvider value={createMemoryViewCache()}>
        <ConfirmProvider>
          <UnreadMessagesContextProvider value={unreadMessages}>
            <UnreadNotificationsContextProvider value={unreadNotifications}>
              {children}
            </UnreadNotificationsContextProvider>
          </UnreadMessagesContextProvider>
        </ConfirmProvider>
      </ViewCacheContextProvider>
    </SessionProvider>
  );
};

export const renderPage = (
  ui: ReactElement,
  options: Readonly<{ path?: string }> = {},
): RenderResult =>
  render(
    <MemoryRouter initialEntries={[options.path ?? "/"]}>
      <AppTestProviders>{ui}</AppTestProviders>
    </MemoryRouter>,
  );

export const cleanupPage = (): void => {
  cleanup();
  resetAnnouncementBannerCache();
};
