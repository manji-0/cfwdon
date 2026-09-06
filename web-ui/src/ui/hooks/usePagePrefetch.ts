import { useEffect, useRef, useState } from "react";
import {
  createPagePrefetch,
  type PagePrefetch,
  type PrefetchItem,
} from "@/ui/lib/page-prefetch";

export const usePagePrefetch = <T extends PrefetchItem>(
  fetchNext: (maxId: string) => Promise<ReadonlyArray<T>>,
): PagePrefetch<T> => {
  const fetchRef = useRef(fetchNext);
  fetchRef.current = fetchNext;
  const [controller] = useState(() =>
    createPagePrefetch<T>((maxId) => fetchRef.current(maxId)),
  );

  useEffect(
    () => () => {
      controller.reset();
    },
    [controller],
  );

  return controller;
};
