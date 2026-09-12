import { Suspense } from "react";
import { Outlet } from "@tanstack/react-router";
import { KeyboardShortcutsHelp } from "@/ui/components/KeyboardShortcutsHelp";
import { SelfProfilePreloader } from "@/ui/components/SelfProfilePreloader";
import { ComposeProvider } from "@/ui/context/ComposeContext";
import { ConfirmProvider } from "@/ui/context/ConfirmContext";
import { UnreadMessagesProvider } from "@/ui/context/UnreadMessagesContext";
import { UnreadNotificationsProvider } from "@/ui/context/UnreadNotificationsContext";
import { ViewCacheProvider } from "@/ui/context/ViewCacheContext";
import { AppKeyboard } from "@/ui/hooks/useAppKeyboard";

const RouteFallback = () => <div className="app-status">読み込み中…</div>;

export const AuthenticatedLayout = () => (
  <ViewCacheProvider>
    <ConfirmProvider>
      <ComposeProvider>
        <SelfProfilePreloader />
        <UnreadMessagesProvider>
          <UnreadNotificationsProvider>
            <KeyboardShortcutsHelp />
            <AppKeyboard />
            <Suspense fallback={<RouteFallback />}>
              <Outlet />
            </Suspense>
          </UnreadNotificationsProvider>
        </UnreadMessagesProvider>
      </ComposeProvider>
    </ConfirmProvider>
  </ViewCacheProvider>
);
