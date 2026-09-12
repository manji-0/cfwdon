import { Link } from "react-router";
import { AppRoute } from "@/domain/navigation/route";

const ME_LINKS = [
  AppRoute.bookmarks(),
  AppRoute.favourites(),
  AppRoute.scheduled(),
  AppRoute.lists(),
  AppRoute.settings(),
] as const;

export const MeHubNav = () => (
  <nav className="me-hub-nav" aria-label="自分">
    {ME_LINKS.map((item) => (
      <Link
        key={item.kind}
        className="app-button app-button-secondary"
        to={AppRoute.toPath(item)}
      >
        {AppRoute.label(item)}
      </Link>
    ))}
  </nav>
);

export const MeBackLink = () => (
  <p className="thread-back">
    <Link to={AppRoute.toPath(AppRoute.profile())}>← 自分</Link>
  </p>
);
