import { NavLink } from "react-router-dom";

export const TimelineTabs = () => (
  <nav className="timeline-tabs" aria-label="タイムライン">
    <NavLink to="/" end className={({ isActive }) => (isActive ? "is-active" : undefined)}>
      ホーム
    </NavLink>
    <NavLink
      to="/public/local"
      className={({ isActive }) => (isActive ? "is-active" : undefined)}
    >
      ローカル
    </NavLink>
    <NavLink to="/public" end className={({ isActive }) => (isActive ? "is-active" : undefined)}>
      連合
    </NavLink>
    <NavLink to="/explore" className={({ isActive }) => (isActive ? "is-active" : undefined)}>
      探索
    </NavLink>
  </nav>
);
