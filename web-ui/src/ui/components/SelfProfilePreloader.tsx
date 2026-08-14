import { useEffect } from "react";
import { loadProfileSnapshot } from "@/application/load-profile-snapshot";
import { CachedView } from "@/domain/cache/cached-view";
import { SelfProfilePreload } from "@/domain/cache/self-profile-preload";
import { useSession } from "@/ui/context/SessionContext";
import { useViewCache } from "@/ui/context/ViewCacheContext";

/** Warm the signed-in account's profile cache before the profile route mounts. */
export const SelfProfilePreloader = () => {
  const { session } = useSession();
  const cache = useViewCache();

  useEffect(() => {
    const cached =
      session.kind === "Authenticated" ? cache.getProfile(session.account.id) : CachedView.absent();
    const decision = SelfProfilePreload.decide(session, cached);
    switch (decision.kind) {
      case "Skip":
        return undefined;
      case "Fetch": {
        const { accountId } = decision;
        let cancelled = false;
        void loadProfileSnapshot(accountId).then((result) => {
          if (cancelled || result.isErr()) {
            return;
          }
          cache.receivePreloadedProfile(accountId, result.value);
        });
        return () => {
          cancelled = true;
        };
      }
    }
  }, [session, cache]);

  return null;
};
