import { useLocation } from "@tanstack/react-router";

export const useAppPathname = (): string =>
  useLocation({ select: (location) => location.pathname });
