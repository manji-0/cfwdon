import type { MouseEvent, ReactNode } from "react";
import { Link } from "@tanstack/react-router";
import { AppRoute } from "@/domain/navigation/route";
import { matchAppLinkTarget } from "@/ui/lib/match-app-link-target";

type AppLinkProps = Readonly<{
  to: AppRoute;
  className?: string | ((state: { isActive: boolean }) => string | undefined);
  children?: ReactNode;
  onClick?: (event: MouseEvent<HTMLAnchorElement>) => void;
  "aria-label"?: string;
  end?: boolean;
}>;

const classNameProps = (className: AppLinkProps["className"]) => {
  if (typeof className === "function") {
    return {
      activeProps: { className: className({ isActive: true }) },
      inactiveProps: { className: className({ isActive: false }) },
    };
  }
  return { className };
};

export const AppLink = ({ to, end, className, children, onClick, "aria-label": ariaLabel }: AppLinkProps) => {
  const rest = {
    ...classNameProps(className),
    children,
    onClick,
    "aria-label": ariaLabel,
    activeOptions: end ? { exact: true as const } : undefined,
  };
  return matchAppLinkTarget(AppRoute.toLink(to), {
    home: () => <Link to={AppRoute.path.home} {...rest} />,
    publicTimeline: () => <Link to={AppRoute.path.publicTimeline} {...rest} />,
    publicTimelineLocal: () => <Link to={AppRoute.path.publicTimelineLocal} {...rest} />,
    tag: (params) => <Link to={AppRoute.path.tag} params={params} {...rest} />,
    notifications: () => <Link to={AppRoute.path.notifications} {...rest} />,
    search: (search) => <Link to={AppRoute.path.search} search={search} {...rest} />,
    profile: () => <Link to={AppRoute.path.profile} {...rest} />,
    account: (params) => <Link to={AppRoute.path.account} params={params} {...rest} />,
    accountFollowers: (params) => (
      <Link to={AppRoute.path.accountFollowers} params={params} {...rest} />
    ),
    accountFollowing: (params) => (
      <Link to={AppRoute.path.accountFollowing} params={params} {...rest} />
    ),
    settings: () => <Link to={AppRoute.path.settings} {...rest} />,
    bookmarks: () => <Link to={AppRoute.path.bookmarks} {...rest} />,
    favourites: () => <Link to={AppRoute.path.favourites} {...rest} />,
    scheduled: () => <Link to={AppRoute.path.scheduled} {...rest} />,
    lists: () => <Link to={AppRoute.path.lists} {...rest} />,
    messages: () => <Link to={AppRoute.path.messages} {...rest} />,
    newMessage: () => <Link to={AppRoute.path.newMessage} {...rest} />,
    conversation: (params) => <Link to={AppRoute.path.conversation} params={params} {...rest} />,
    status: (params) => <Link to={AppRoute.path.status} params={params} {...rest} />,
    statusHistory: (params) => <Link to={AppRoute.path.statusHistory} params={params} {...rest} />,
    statusFavouritedBy: (params) => (
      <Link to={AppRoute.path.statusFavouritedBy} params={params} {...rest} />
    ),
    statusRebloggedBy: (params) => (
      <Link to={AppRoute.path.statusRebloggedBy} params={params} {...rest} />
    ),
    statusQuotes: (params) => <Link to={AppRoute.path.statusQuotes} params={params} {...rest} />,
  });
};
