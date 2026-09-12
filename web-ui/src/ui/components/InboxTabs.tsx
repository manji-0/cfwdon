import { AppRoute } from "@/domain/navigation/route";
import { AppLink } from "@/ui/lib/app-link";

export const InboxTabs = () => (
  <nav className="timeline-tabs" aria-label="受信">
    <AppLink
      to={AppRoute.notifications()}
      end
      className={({ isActive }) => (isActive ? "is-active" : undefined)}
    >
      通知
    </AppLink>
    <AppLink
      to={AppRoute.messages()}
      className={({ isActive }) => (isActive ? "is-active" : undefined)}
    >
      メッセージ
    </AppLink>
  </nav>
);
