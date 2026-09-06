import { Suspense, useEffect, useState } from "react";
import { BrowserRouter, Navigate, Route, Routes } from "react-router-dom";
import { loadSession } from "@/application/load-session";
import { SessionState } from "@/domain/session/session";
import { KeyboardShortcutsHelp } from "@/ui/components/KeyboardShortcutsHelp";
import { LoginPanel } from "@/ui/components/LoginPanel";
import { SelfProfilePreloader } from "@/ui/components/SelfProfilePreloader";
import { SessionProvider, createSessionContextValue } from "@/ui/context/SessionContext";
import { UnreadMessagesProvider } from "@/ui/context/UnreadMessagesContext";
import { UnreadNotificationsProvider } from "@/ui/context/UnreadNotificationsContext";
import { ViewCacheProvider } from "@/ui/context/ViewCacheContext";
import { HomePage } from "@/ui/pages/HomePage";
import {
  BookmarksPage,
  ConversationPage,
  ExplorePage,
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
        <SelfProfilePreloader />
        <UnreadMessagesProvider>
          <UnreadNotificationsProvider>
          <KeyboardShortcutsHelp />
          <Suspense fallback={<RouteFallback />}>
            <Routes>
              <Route path="/" element={<HomePage />} />
              <Route path="/public" element={<PublicTimelinePage />} />
              <Route path="/public/local" element={<PublicTimelinePage />} />
              <Route path="/tags/:tagName" element={<TagTimelinePage />} />
              <Route path="/explore" element={<ExplorePage />} />
              <Route path="/status/:statusId/history" element={<StatusHistoryPage />} />
              <Route path="/status/:statusId/favourited-by" element={<StatusFavouritedByPage />} />
              <Route path="/status/:statusId/reblogged-by" element={<StatusRebloggedByPage />} />
              <Route path="/status/:statusId/quotes" element={<StatusQuotesPage />} />
              <Route path="/status/:statusId" element={<ThreadPage />} />
              <Route path="/profile" element={<ProfilePage />} />
              <Route path="/profile/:accountId/followers" element={<AccountFollowersPage />} />
              <Route path="/profile/:accountId/following" element={<AccountFollowingPage />} />
              <Route path="/profile/:accountId" element={<ProfilePage />} />
              <Route path="/notifications" element={<NotificationsPage />} />
              <Route path="/search" element={<SearchPage />} />
              <Route path="/settings" element={<SettingsPage />} />
              <Route path="/bookmarks" element={<BookmarksPage />} />
              <Route path="/favourites" element={<FavouritesPage />} />
              <Route path="/scheduled" element={<ScheduledStatusesPage />} />
              <Route path="/lists" element={<ListsPage />} />
              <Route path="/messages" element={<MessagesPage />} />
              <Route path="/messages/new" element={<NewMessagePage />} />
              <Route path="/messages/:conversationId" element={<ConversationPage />} />
              <Route path="*" element={<Navigate to="/" replace />} />
            </Routes>
          </Suspense>
          </UnreadNotificationsProvider>
        </UnreadMessagesProvider>
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
    <BrowserRouter basename="/app">
      <AppRoutes session={session} setSession={setSession} />
    </BrowserRouter>
  );
};
