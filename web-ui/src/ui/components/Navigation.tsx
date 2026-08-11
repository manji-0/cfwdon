import { NavLink } from "react-router-dom";
import { AppRoute } from "@/domain/navigation/route";
import { IconBell, IconGear, IconHome, IconSearch, IconUser } from "@/ui/components/icons";

const PRIMARY_NAV_ITEMS = [
  { route: AppRoute.home(), icon: IconHome, end: true },
  { route: AppRoute.notifications(), icon: IconBell, end: false },
  { route: AppRoute.search(), icon: IconSearch, end: false },
  { route: AppRoute.profile(), icon: IconUser, end: false },
  { route: AppRoute.settings(), icon: IconGear, end: false },
] as const;

const LIBRARY_NAV_ITEMS = [
  AppRoute.bookmarks(),
  AppRoute.lists(),
  AppRoute.messages(),
] as const;

export const SidebarNav = () => (
  <nav className="app-nav" aria-label="メイン">
    <div className="app-brand">
      <span className="app-brand-mark" aria-hidden="true">
        C
      </span>
    </div>
    {PRIMARY_NAV_ITEMS.map(({ route, icon: Icon, end }) => (
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
    <div className="app-nav-library" aria-label="ライブラリ">
      {LIBRARY_NAV_ITEMS.map((route) => (
        <NavLink
          key={route.kind}
          to={AppRoute.toPath(route)}
          className={({ isActive }) => `app-nav-library-link${isActive ? " is-active" : ""}`}
        >
          {AppRoute.label(route)}
        </NavLink>
      ))}
    </div>
  </nav>
);

export const BottomNav = () => (
  <nav className="app-bottom-nav" aria-label="モバイルナビ">
    {PRIMARY_NAV_ITEMS.map(({ route, icon: Icon, end }) => (
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
