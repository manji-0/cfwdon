/**
 * Web UI delivery phases for cfwdon `/app`.
 *
 * Keep TODO comments in skeleton modules aligned with these ids.
 */
export const WebUiPhase = {
  shell: "Phase 0",
  timeline: "Phase 1",
  timelineMedia: "Phase 1 (media)",
  notificationsSearch: "Phase 2",
  settings: "Phase 3",
  streaming: "Phase 4",
  collections: "Phase 5",
} as const;

export type WebUiPhaseId = (typeof WebUiPhase)[keyof typeof WebUiPhase];

export const WebUiPhaseSummary = {
  [WebUiPhase.shell]: "App shell, session, navigation, login",
  [WebUiPhase.timeline]: "Home timeline, composer, thread, profile",
  [WebUiPhase.timelineMedia]: "Composer media upload and home trends sidebar",
  [WebUiPhase.notificationsSearch]: "Notifications list and federated search",
  [WebUiPhase.settings]: "Account preferences, filters, and logout",
  [WebUiPhase.streaming]: "Live timeline and notification updates",
  [WebUiPhase.collections]: "Bookmarks, lists, and direct messages",
} as const satisfies Record<WebUiPhaseId, string>;
