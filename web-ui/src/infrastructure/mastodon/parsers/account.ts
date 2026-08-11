import type { AccountProfile, AccountRef } from "@/domain/account/account";
import type { AccountCredentials } from "@/domain/account/credentials";
import type { AccountSummary } from "@/domain/session/account";
import { mastodon } from "@/infrastructure/mastodon/parsers/definitions";

export const parseAccountRef = mastodon.type("AccountRefApi").pipe(
  (value): AccountRef => ({
    id: value.id,
    username: value.username,
    acct: value.acct,
    displayName: value.display_name,
    avatar: value.avatar,
  }),
);

export const parseAccountProfile = mastodon.type("AccountProfileApi").pipe(
  (value): AccountProfile => ({
    id: value.id,
    username: value.username,
    acct: value.acct,
    displayName: value.display_name,
    avatar: value.avatar,
    header: value.header,
    note: value.note,
    followersCount: value.followers_count,
    followingCount: value.following_count,
    statusesCount: value.statuses_count,
    locked: value.locked,
  }),
);

export const parseAccountCredentials = mastodon.type("AccountCredentialsApi").pipe(
  (value): AccountCredentials => ({
    id: value.id,
    displayName: value.display_name,
    note: value.note,
    avatar: value.avatar,
    username: value.username,
    acct: value.acct,
    source: {
      privacy: value.source.privacy,
      sensitive: value.source.sensitive,
      language: value.source.language,
      quotePolicy: value.source.quote_policy,
    },
  }),
);

export const parseAccountSummary = mastodon.type("AccountSummaryApi").pipe(
  (value): AccountSummary => ({
    id: value.id,
    username: value.username,
    displayName: value.display_name,
    acct: value.acct,
    avatar: value.avatar,
    instanceName: value.instance_name,
  }),
);
