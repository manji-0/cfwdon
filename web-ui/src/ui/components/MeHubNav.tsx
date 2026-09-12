import { AppRoute } from "@/domain/navigation/route";
import { AppLink } from "@/ui/lib/app-link";

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
      <AppLink key={item.kind} className="app-button app-button-secondary" to={item}>
        {AppRoute.label(item)}
      </AppLink>
    ))}
  </nav>
);

export const MeBackLink = () => (
  <p className="thread-back">
    <AppLink to={AppRoute.profile()}>← 自分</AppLink>
  </p>
);
