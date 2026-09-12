import {
  Navigate,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  redirect,
  type AnyRouter,
  type RouteComponent,
} from "@tanstack/react-router";
import { AppRoute } from "@/domain/navigation/route";
import { SearchType } from "@/domain/search/search";
import { AuthenticatedLayout } from "@/ui/AuthenticatedLayout";
import { HomePage } from "@/ui/pages/HomePage";
import {
  AccountFollowersPage,
  AccountFollowingPage,
  BookmarksPage,
  ConversationPage,
  FavouritesPage,
  ListsPage,
  MessagesPage,
  NewMessagePage,
  NotificationsPage,
  ProfilePage,
  PublicTimelinePage,
  ScheduledStatusesPage,
  SearchPage,
  SettingsPage,
  StatusFavouritedByPage,
  StatusHistoryPage,
  StatusQuotesPage,
  StatusRebloggedByPage,
  TagTimelinePage,
  ThreadPage,
} from "@/ui/pages/lazy-pages";

const parseSearch = (search: Record<string, unknown>) => ({
  q: typeof search.q === "string" ? search.q : "",
  type: SearchType.fromParam(typeof search.type === "string" ? search.type : null),
});

const NotFoundRedirect = () => <Navigate to={AppRoute.path.home} replace />;

const PublicFederatedPage = () => <PublicTimelinePage local={false} />;
const PublicLocalPage = () => <PublicTimelinePage local />;

export const buildRouteTree = (RootComponent: RouteComponent) => {
  const rootRoute = createRootRoute({
    component: RootComponent,
  });

  const child = (path: string, component: RouteComponent) =>
    createRoute({
      getParentRoute: () => rootRoute,
      path,
      component,
    });

  const searchRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: AppRoute.path.search,
    validateSearch: parseSearch,
    component: SearchPage,
  });

  const exploreRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: AppRoute.path.explore,
    beforeLoad: () => {
      throw redirect({ to: AppRoute.path.home });
    },
  });

  return rootRoute.addChildren([
    child(AppRoute.path.home, HomePage),
    child(AppRoute.path.publicTimeline, PublicFederatedPage),
    child(AppRoute.path.publicTimelineLocal, PublicLocalPage),
    child(AppRoute.path.tag, TagTimelinePage),
    exploreRoute,
    child(AppRoute.path.statusHistory, StatusHistoryPage),
    child(AppRoute.path.statusFavouritedBy, StatusFavouritedByPage),
    child(AppRoute.path.statusRebloggedBy, StatusRebloggedByPage),
    child(AppRoute.path.statusQuotes, StatusQuotesPage),
    child(AppRoute.path.status, ThreadPage),
    child(AppRoute.path.profile, ProfilePage),
    child(AppRoute.path.accountFollowers, AccountFollowersPage),
    child(AppRoute.path.accountFollowing, AccountFollowingPage),
    child(AppRoute.path.account, ProfilePage),
    child(AppRoute.path.notifications, NotificationsPage),
    searchRoute,
    child(AppRoute.path.settings, SettingsPage),
    child(AppRoute.path.bookmarks, BookmarksPage),
    child(AppRoute.path.favourites, FavouritesPage),
    child(AppRoute.path.scheduled, ScheduledStatusesPage),
    child(AppRoute.path.lists, ListsPage),
    child(AppRoute.path.messages, MessagesPage),
    child(AppRoute.path.newMessage, NewMessagePage),
    child(AppRoute.path.conversation, ConversationPage),
  ]);
};

export const createAppRouter = (options: {
  history?: ReturnType<typeof createMemoryHistory>;
  basepath?: string;
  RootComponent?: RouteComponent;
} = {}): AnyRouter =>
  createRouter({
    routeTree: buildRouteTree(options.RootComponent ?? AuthenticatedLayout),
    history: options.history,
    basepath: options.basepath ?? AppRoute.basename,
    defaultNotFoundComponent: NotFoundRedirect,
  });

export const appRouter = createAppRouter();

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof appRouter;
  }
}
