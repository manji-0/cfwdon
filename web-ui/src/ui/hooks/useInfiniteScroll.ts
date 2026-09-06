import { useEffect, useRef, type RefObject } from "react";

const ROOT_MARGIN = "0px 0px 320px 0px";

type InfiniteScrollOptions = Readonly<{
  enabled: boolean;
  onLoadMore: () => void;
  observeKey?: string | number;
}>;

/** Observe a sentinel and call `onLoadMore` when it nears the viewport bottom. */
export const useInfiniteScroll = <T extends Element>(
  options: InfiniteScrollOptions,
): RefObject<T | null> => {
  const sentinelRef = useRef<T | null>(null);
  const onLoadMoreRef = useRef(options.onLoadMore);
  const inFlightRef = useRef(false);
  onLoadMoreRef.current = options.onLoadMore;

  useEffect(() => {
    if (!options.enabled) {
      inFlightRef.current = false;
      return;
    }
    const node = sentinelRef.current;
    if (!node) {
      return;
    }
    inFlightRef.current = false;
    const observer = new IntersectionObserver(
      (entries) => {
        if (inFlightRef.current || !entries.some((entry) => entry.isIntersecting)) {
          return;
        }
        inFlightRef.current = true;
        onLoadMoreRef.current();
      },
      { rootMargin: ROOT_MARGIN },
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, [options.enabled, options.observeKey]);

  return sentinelRef;
};
