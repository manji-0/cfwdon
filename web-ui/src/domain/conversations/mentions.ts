import type { AccountRef } from "@/domain/account/account";

const mentionPattern = (acct: string): RegExp =>
  new RegExp(`(^|\\s)@${acct.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}(\\s|$)`, "i");

/** Prefix missing `@acct` mentions so Mastodon delivers the direct message. */
export const ensureDirectMentions = (
  text: string,
  participants: ReadonlyArray<Pick<AccountRef, "acct">>,
): string => {
  const trimmed = text.trim();
  const missing = participants
    .map((participant) => participant.acct.trim())
    .filter((acct) => acct.length > 0)
    .filter((acct) => !mentionPattern(acct).test(trimmed));
  if (missing.length === 0) {
    return trimmed;
  }
  return `${missing.map((acct) => `@${acct}`).join(" ")} ${trimmed}`.trim();
};
