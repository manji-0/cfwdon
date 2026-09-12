import { Suspense, useEffect, useState } from "react";
import { BrowserRouter, Navigate, Route, Routes } from "react-router";
import { loadSession } from "@/application/load-session";
import { AppRoute } from "@/domain/navigation/route";
import { SessionState } from "@/domain/session/session";
import { KeyboardShortcutsHelp } from "@/ui/components/KeyboardShortcutsHelp";
import { LoginPanel } from "@/ui/components/LoginPanel";
import { SelfProfilePreloader } from "@/ui/components/SelfProfilePreloader";
import { ComposeProvider } from "@/ui/context/ComposeContext";
import { ConfirmProvider } from "@/ui/context/ConfirmContext";
import { SessionProvider, createSessionContextValue } from "@/ui/context/SessionContext";
import { UnreadMessagesProvider } from "@/ui/context/UnreadMessagesContext";
import { UnreadNotificationsProvider } from "@/ui/context/UnreadNotificationsContext";
import { ViewCacheProvider } from "@/ui/context/ViewCacheContext";
import { AppKeyboard } from "@/ui/hooks/useAppKeyboard";
import { HomePage } from "@/ui/pages/HomePage";
import {
  BookmarksPage,
  ConversationPage,
  FavouritesPage,
  ListsPage,
  MessagesPage,
  NewMessagePage,
  NotificationsPage,
  ProfilePage,
  PublicTimelinePage,
  SearchPage,
  SettingsPage,
  StatusHistoryPage,
  TagTimelinePage,
  ThreadPage,
  AccountFollowersPage,
  AccountFollowingPage,
  StatusFavouritedByPage,
  StatusRebloggedByPage,
  StatusQuotesPage,
  ScheduledStatusesPage,
} from "@/ui/pages/lazy-pages";
import "@/ui/styles/app.css";

const RouteFallback = () => <div className="app-status">読み込み中…</div>;

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
      <ViewCacheProvider>
        <ConfirmProvider>
          <ComposeProvider>
            <SelfProfilePreloader />
            <UnreadMessagesProvider>
              <UnreadNotificationsProvider>
                <KeyboardShortcutsHelp />
                <AppKeyboard />
                <Suspense fallback={<RouteFallback />}>
                  <Routes>
                    <Route path={AppRoute.pattern.home} element={<HomePage />} />
                    <Route path={AppRoute.pattern.publicTimeline} element={<PublicTimelinePage />} />
                    <Route
                      path={AppRoute.pattern.publicTimelineLocal}
                      element={<PublicTimelinePage />}
                    />
                    <Route path={AppRoute.pattern.tag} element={<TagTimelinePage />} />
                    <Route
                      path={AppRoute.pattern.explore}
                      element={<Navigate to={AppRoute.toPath(AppRoute.home())} replace />}
                    />
                    <Route path={AppRoute.pattern.statusHistory} element={<StatusHistoryPage />} />
                    <Route
                      path={AppRoute.pattern.statusFavouritedBy}
                      element={<StatusFavouritedByPage />}
                    />
                    <Route
                      path={AppRoute.pattern.statusRebloggedBy}
                      element={<StatusRebloggedByPage />}
                    />
                    <Route path={AppRoute.pattern.statusQuotes} element={<StatusQuotesPage />} />
                    <Route path={AppRoute.pattern.status} element={<ThreadPage />} />
                    <Route path={AppRoute.pattern.profile} element={<ProfilePage />} />
                    <Route
                      path={AppRoute.pattern.accountFollowers}
                      element={<AccountFollowersPage />}
                    />
                    <Route
                      path={AppRoute.pattern.accountFollowing}
                      element={<AccountFollowingPage />}
                    />
                    <Route path={AppRoute.pattern.account} element={<ProfilePage />} />
                    <Route path={AppRoute.pattern.notifications} element={<NotificationsPage />} />
                    <Route path={AppRoute.pattern.search} element={<SearchPage />} />
                    <Route path={AppRoute.pattern.settings} element={<SettingsPage />} />
                    <Route path={AppRoute.pattern.bookmarks} element={<BookmarksPage />} />
                    <Route path={AppRoute.pattern.favourites} element={<FavouritesPage />} />
                    <Route path={AppRoute.pattern.scheduled} element={<ScheduledStatusesPage />} />
                    <Route path={AppRoute.pattern.lists} element={<ListsPage />} />
                    <Route path={AppRoute.pattern.messages} element={<MessagesPage />} />
                    <Route path={AppRoute.pattern.newMessage} element={<NewMessagePage />} />
                    <Route path={AppRoute.pattern.conversation} element={<ConversationPage />} />
                    <Route path="*" element={<Navigate to={AppRoute.toPath(AppRoute.home())} replace />} />
                  </Routes>
                </Suspense>
              </UnreadNotificationsProvider>
            </UnreadMessagesProvider>
          </ComposeProvider>
        </ConfirmProvider>
      </ViewCacheProvider>
    </SessionProvider>
  );
};

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

  return (
    <BrowserRouter basename={AppRoute.basename}>
      <AppRoutes session={session} setSession={setSession} />
    </BrowserRouter>
  );
};
