import { NavLink } from "react-router-dom";
import { AppRoute } from "@/domain/navigation/route";
import { IconBell, IconGear, IconHome, IconSearch, IconUser } from "@/ui/components/icons";

const NAV_ITEMS = [
  { route: AppRoute.home(), icon: IconHome, end: true },
  { route: AppRoute.notifications(), icon: IconBell, end: false },
  { route: AppRoute.search(), icon: IconSearch, end: false },
  { route: AppRoute.profile(), icon: IconUser, end: false },
  { route: AppRoute.settings(), icon: IconGear, end: false },
] as const;

export const SidebarNav = () => (
  <nav className="app-nav" aria-label="メイン">
    <div className="app-brand">
      <span className="app-brand-mark" aria-hidden="true">
        C
      </span>
    </div>
    {NAV_ITEMS.map(({ route, icon: Icon, end }) => (
      <NavLink
        key={route.kind}
        to={AppRoute.toPath(route)}
        end={end}
        className={({ isActive }) => `app-nav-link${isActive ? " is-active" : ""}`}
        aria-label={AppRoute.label(route)}
      >
        <Icon aria-hidden="true" />
      </NavLink>
    ))}
  </nav>
);

export const BottomNav = () => (
  <nav className="app-bottom-nav" aria-label="モバイルナビ">
    {NAV_ITEMS.map(({ route, icon: Icon, end }) => (
      <NavLink
        key={route.kind}
        to={AppRoute.toPath(route)}
        end={end}
        className={({ isActive }) => (isActive ? "is-active" : undefined)}
        aria-label={AppRoute.label(route)}
      >
        <Icon aria-hidden="true" />
        <span>{AppRoute.label(route)}</span>
      </NavLink>
    ))}
  </nav>
);
