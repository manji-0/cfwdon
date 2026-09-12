import type { ComponentType, MouseEvent, ReactNode } from "react";
import { Link } from "@tanstack/react-router";
import { AppRoute, type AppLinkTarget } from "@/domain/navigation/route";

type AppLinkProps = Readonly<{
  to: AppRoute;
  className?: string | ((state: { isActive: boolean }) => string | undefined);
  children?: ReactNode;
  onClick?: (event: MouseEvent<HTMLAnchorElement>) => void;
  "aria-label"?: string;
  end?: boolean;
}>;

const RouterLink = Link as unknown as ComponentType<
  AppLinkTarget &
    Omit<AppLinkProps, "to" | "end"> &
    Readonly<{ activeOptions?: { exact: boolean } }>
>;

export const AppLink = ({ to, end, ...rest }: AppLinkProps) => {
  const target = AppRoute.toLink(to);
  return <RouterLink {...target} {...rest} activeOptions={end ? { exact: true } : undefined} />;
};
