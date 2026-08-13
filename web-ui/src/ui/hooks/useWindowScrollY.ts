import { useEffect, useRef } from "react";

/** Track window scroll so cache can restore it after SPA navigations. */
export const useWindowScrollY = () => {
  const scrollYRef = useRef(0);

  useEffect(() => {
    const onScroll = () => {
      scrollYRef.current = window.scrollY;
    };
    onScroll();
    window.addEventListener("scroll", onScroll, { passive: true });
    return () => window.removeEventListener("scroll", onScroll);
  }, []);

  return scrollYRef;
};
