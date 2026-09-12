import { useNavigate } from "@tanstack/react-router";
import { AppRoute } from "@/domain/navigation/route";
import { matchAppLinkTarget } from "@/ui/lib/match-app-link-target";

export const useAppNavigate = () => {
  const navigate = useNavigate();
  return (route: AppRoute, options: Readonly<{ replace?: boolean }> = {}) => {
    const replace = options.replace;
    void matchAppLinkTarget(AppRoute.toLink(route), {
      home: () => navigate({ to: AppRoute.path.home, replace }),
      publicTimeline: () => navigate({ to: AppRoute.path.publicTimeline, replace }),
      publicTimelineLocal: () => navigate({ to: AppRoute.path.publicTimelineLocal, replace }),
      tag: (params) => navigate({ to: AppRoute.path.tag, params, replace }),
      notifications: () => navigate({ to: AppRoute.path.notifications, replace }),
      search: (search) => navigate({ to: AppRoute.path.search, search, replace }),
      profile: () => navigate({ to: AppRoute.path.profile, replace }),
      account: (params) => navigate({ to: AppRoute.path.account, params, replace }),
      accountFollowers: (params) =>
        navigate({ to: AppRoute.path.accountFollowers, params, replace }),
      accountFollowing: (params) =>
        navigate({ to: AppRoute.path.accountFollowing, params, replace }),
      settings: () => navigate({ to: AppRoute.path.settings, replace }),
      bookmarks: () => navigate({ to: AppRoute.path.bookmarks, replace }),
      favourites: () => navigate({ to: AppRoute.path.favourites, replace }),
      scheduled: () => navigate({ to: AppRoute.path.scheduled, replace }),
      lists: () => navigate({ to: AppRoute.path.lists, replace }),
      messages: () => navigate({ to: AppRoute.path.messages, replace }),
      newMessage: () => navigate({ to: AppRoute.path.newMessage, replace }),
      conversation: (params) => navigate({ to: AppRoute.path.conversation, params, replace }),
      status: (params) => navigate({ to: AppRoute.path.status, params, replace }),
      statusHistory: (params) => navigate({ to: AppRoute.path.statusHistory, params, replace }),
      statusFavouritedBy: (params) =>
        navigate({ to: AppRoute.path.statusFavouritedBy, params, replace }),
      statusRebloggedBy: (params) =>
        navigate({ to: AppRoute.path.statusRebloggedBy, params, replace }),
      statusQuotes: (params) => navigate({ to: AppRoute.path.statusQuotes, params, replace }),
    });
  };
};
