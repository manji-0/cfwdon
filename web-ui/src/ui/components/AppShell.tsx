import type { ReactNode } from "react";
import { AnnouncementBanner } from "@/ui/components/AnnouncementBanner";
import { BottomNav, SidebarNav } from "@/ui/components/Navigation";

type AppShellProps = Readonly<{
  title: string;
  aside?: ReactNode;
  children: ReactNode;
}>;

export const AppShell = ({ title, aside, children }: AppShellProps) => (
  <div className="app-shell">
    <SidebarNav />
    <div className="app-main">
      <header className="app-main-header">{title}</header>
      <AnnouncementBanner />
      <div className="app-main-body">{children}</div>
    </div>
    <aside className="app-aside" aria-label="サイドバー">
      {aside}
    </aside>
    <BottomNav />
  </div>
);
