import type { ReactNode } from "react";
import { AnnouncementBanner } from "@/ui/components/AnnouncementBanner";
import { BottomNav, SidebarNav } from "@/ui/components/Navigation";

type AppShellProps = Readonly<{
  title: string;
  children: ReactNode;
}>;

export const AppShell = ({ title, children }: AppShellProps) => (
  <div className="app-shell">
    <SidebarNav />
    <div className="app-main">
      <header className="app-main-header">{title}</header>
      <AnnouncementBanner />
      <div className="app-main-body">{children}</div>
    </div>
    <BottomNav />
  </div>
);
