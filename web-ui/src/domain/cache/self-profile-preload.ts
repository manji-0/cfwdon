import type { SessionState } from "@/domain/session/session";
import type { CachedView } from "./cached-view";
import type { ProfileSnapshot } from "./profile-set";

export type SelfProfilePreloadSkip = Readonly<{
  kind: "Skip";
  reason: "NotAuthenticated" | "AlreadyCached";
}>;

export type SelfProfilePreloadFetch = Readonly<{
  kind: "Fetch";
  accountId: string;
}>;

export type SelfProfilePreload = SelfProfilePreloadSkip | SelfProfilePreloadFetch;

export const SelfProfilePreload = {
  skip: (reason: SelfProfilePreloadSkip["reason"]): SelfProfilePreloadSkip => ({
    kind: "Skip",
    reason,
  }),

  fetch: (accountId: string): SelfProfilePreloadFetch => ({
    kind: "Fetch",
    accountId,
  }),

  decide: (session: SessionState, cached: CachedView<ProfileSnapshot>): SelfProfilePreload => {
    switch (session.kind) {
      case "Anonymous":
      case "Loading":
      case "Failed":
        return SelfProfilePreload.skip("NotAuthenticated");
      case "Authenticated":
        switch (cached.kind) {
          case "Present":
            return SelfProfilePreload.skip("AlreadyCached");
          case "Absent":
            return SelfProfilePreload.fetch(session.account.id);
        }
    }
  },
} as const;
