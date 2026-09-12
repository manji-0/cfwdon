import { useNavigate } from "@tanstack/react-router";
import { AppRoute } from "@/domain/navigation/route";

export const useAppNavigate = () => {
  const navigate = useNavigate();
  return (route: AppRoute, options: Readonly<{ replace?: boolean }> = {}) => {
    void navigate({ ...AppRoute.toLink(route), replace: options.replace } as never);
  };
};
