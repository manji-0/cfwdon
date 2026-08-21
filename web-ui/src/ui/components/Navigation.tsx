import { NavLink } from "react-router-dom";
import { AppRoute } from "@/domain/navigation/route";
import {
  IconBell,
  IconBookmark,
  IconBrand,
  IconGear,
  IconHeart,
  IconHome,
  IconList,
  IconMessage,
  IconSearch,
  IconUser,
} from "@/ui/components/icons";
import { useUnreadMessages } from "@/ui/context/UnreadMessagesContext";

const PRIMARY_NAV_ITEMS = [
  { route: AppRoute.home(), icon: IconHome, end: true },
  { route: AppRoute.notifications(), icon: IconBell, end: false },
  { route: AppRoute.search(), icon: IconSearch, end: false },
  { route: AppRoute.profile(), icon: IconUser, end: false },
  { route: AppRoute.settings(), icon: IconGear, end: false },
] as const;

const LIBRARY_NAV_ITEMS = [
  { route: AppRoute.bookmarks(), icon: IconBookmark },
  { route: AppRoute.favourites(), icon: IconHeart },
  { route: AppRoute.lists(), icon: IconList },
  { route: AppRoute.messages(), icon: IconMessage },
] as const;

export const SidebarNav = () => {
  const { unreadCount } = useUnreadMessages();

  return (
    <nav className="app-nav" aria-label="メイン">
      <NavLink
        to={AppRoute.toPath(AppRoute.home())}
        end
        className="app-brand"
        aria-label="cfwdon ホーム"
      >
        <span className="app-brand-mark" aria-hidden="true">
          <IconBrand />
        </span>
      </NavLink>
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
        {LIBRARY_NAV_ITEMS.map(({ route, icon: Icon }) => {
          const isMessages = route.kind === "Messages";
          const label = AppRoute.label(route);
          return (
            <NavLink
              key={route.kind}
              to={AppRoute.toPath(route)}
              className={({ isActive }) => `app-nav-link${isActive ? " is-active" : ""}`}
              aria-label={
                isMessages && unreadCount > 0 ? `${label}（未読 ${unreadCount}）` : label
              }
            >
              <Icon aria-hidden="true" />
              {isMessages && unreadCount > 0 ? (
                <span className="nav-unread-badge" aria-hidden="true">
                  {unreadCount > 99 ? "99+" : unreadCount}
                </span>
              ) : null}
            </NavLink>
          );
        })}
      </div>
    </nav>
  );
};

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
