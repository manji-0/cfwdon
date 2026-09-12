import { NavLink } from "react-router";
import { AppRoute } from "@/domain/navigation/route";

export const InboxTabs = () => (
  <nav className="timeline-tabs" aria-label="受信">
    <NavLink
      to={AppRoute.toPath(AppRoute.notifications())}
      end
      className={({ isActive }) => (isActive ? "is-active" : undefined)}
    >
      通知
    </NavLink>
    <NavLink
      to={AppRoute.toPath(AppRoute.messages())}
      className={({ isActive }) => (isActive ? "is-active" : undefined)}
    >
      メッセージ
    </NavLink>
  </nav>
);
