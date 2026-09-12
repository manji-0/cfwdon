import { useSearch } from "@tanstack/react-router";
import { AppRoute } from "@/domain/navigation/route";
import { SearchType } from "@/domain/search/search";

export const useAppSearch = () => {
  const search = useSearch({ from: AppRoute.path.search });
  return {
    q: typeof search.q === "string" ? search.q : "",
    type: SearchType.fromParam(typeof search.type === "string" ? search.type : null),
  };
};
