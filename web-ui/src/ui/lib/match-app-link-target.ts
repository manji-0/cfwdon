import { AppRoute, type AppLinkTarget } from "@/domain/navigation/route";
import type { SearchType } from "@/domain/search/search";

type SearchLink = Readonly<{
  q: string;
  type: SearchType;
}>;

export type AppLinkVisitor<T> = Readonly<{
  home: () => T;
  publicTimeline: () => T;
  publicTimelineLocal: () => T;
  tag: (params: Readonly<{ tagName: string }>) => T;
  notifications: () => T;
  search: (search: SearchLink) => T;
  profile: () => T;
  account: (params: Readonly<{ accountId: string }>) => T;
  accountFollowers: (params: Readonly<{ accountId: string }>) => T;
  accountFollowing: (params: Readonly<{ accountId: string }>) => T;
  settings: () => T;
  bookmarks: () => T;
  favourites: () => T;
  scheduled: () => T;
  lists: () => T;
  messages: () => T;
  newMessage: () => T;
  conversation: (params: Readonly<{ conversationId: string }>) => T;
  status: (params: Readonly<{ statusId: string }>) => T;
  statusHistory: (params: Readonly<{ statusId: string }>) => T;
  statusFavouritedBy: (params: Readonly<{ statusId: string }>) => T;
  statusRebloggedBy: (params: Readonly<{ statusId: string }>) => T;
  statusQuotes: (params: Readonly<{ statusId: string }>) => T;
}>;

export const matchAppLinkTarget = <T>(target: AppLinkTarget, visitor: AppLinkVisitor<T>): T => {
  switch (target.to) {
    case AppRoute.path.home:
      return visitor.home();
    case AppRoute.path.publicTimeline:
      return visitor.publicTimeline();
    case AppRoute.path.publicTimelineLocal:
      return visitor.publicTimelineLocal();
    case AppRoute.path.tag:
      return visitor.tag(target.params);
    case AppRoute.path.notifications:
      return visitor.notifications();
    case AppRoute.path.search:
      return visitor.search(target.search);
    case AppRoute.path.profile:
      return visitor.profile();
    case AppRoute.path.account:
      return visitor.account(target.params);
    case AppRoute.path.accountFollowers:
      return visitor.accountFollowers(target.params);
    case AppRoute.path.accountFollowing:
      return visitor.accountFollowing(target.params);
    case AppRoute.path.settings:
      return visitor.settings();
    case AppRoute.path.bookmarks:
      return visitor.bookmarks();
    case AppRoute.path.favourites:
      return visitor.favourites();
    case AppRoute.path.scheduled:
      return visitor.scheduled();
    case AppRoute.path.lists:
      return visitor.lists();
    case AppRoute.path.messages:
      return visitor.messages();
    case AppRoute.path.newMessage:
      return visitor.newMessage();
    case AppRoute.path.conversation:
      return visitor.conversation(target.params);
    case AppRoute.path.status:
      return visitor.status(target.params);
    case AppRoute.path.statusHistory:
      return visitor.statusHistory(target.params);
    case AppRoute.path.statusFavouritedBy:
      return visitor.statusFavouritedBy(target.params);
    case AppRoute.path.statusRebloggedBy:
      return visitor.statusRebloggedBy(target.params);
    case AppRoute.path.statusQuotes:
      return visitor.statusQuotes(target.params);
    default: {
      const missed: never = target;
      return missed;
    }
  }
};
