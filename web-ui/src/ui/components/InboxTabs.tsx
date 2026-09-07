import { NavLink } from "react-router";

export const InboxTabs = () => (
  <nav className="timeline-tabs" aria-label="受信">
    <NavLink to="/notifications" end className={({ isActive }) => (isActive ? "is-active" : undefined)}>
      通知
    </NavLink>
    <NavLink to="/messages" className={({ isActive }) => (isActive ? "is-active" : undefined)}>
      メッセージ
    </NavLink>
  </nav>
);
