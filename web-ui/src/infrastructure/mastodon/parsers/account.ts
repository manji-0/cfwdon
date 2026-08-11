import { type } from "arktype";
import type { AccountProfile, AccountRef } from "@/domain/account/account";
import type { AccountCredentials } from "@/domain/account/credentials";
import type { AccountSummary } from "@/domain/session/account";

const AccountRefParser = type({
  id: "string>0",
  username: "string>0",
  acct: "string>0",
  display_name: "string",
  avatar: "string",
}).pipe(
  (value): AccountRef => ({
    id: value.id,
    username: value.username,
    acct: value.acct,
    displayName: value.display_name,
    avatar: value.avatar,
  }),
);

const AccountProfileParser = type({
  id: "string>0",
  username: "string>0",
  acct: "string>0",
  display_name: "string",
  avatar: "string",
  header: "string",
  note: "string",
  followers_count: "number",
  following_count: "number",
  statuses_count: "number",
  locked: "boolean",
}).pipe(
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

const AccountCredentialsParser = type({
  id: "string>0",
  display_name: "string",
  note: "string",
  avatar: "string",
  username: "string>0",
  acct: "string>0",
  source: {
    privacy: "string",
    sensitive: "boolean",
    language: "string | null",
    quote_policy: "string",
  },
}).pipe(
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

const AccountSummaryParser = type({
  id: "string>0",
  username: "string>0",
  display_name: "string",
  acct: "string>0",
  avatar: "string",
  instance_name: "string>0",
}).pipe(
  (value): AccountSummary => ({
    id: value.id,
    username: value.username,
    displayName: value.display_name,
    acct: value.acct,
    avatar: value.avatar,
    instanceName: value.instance_name,
  }),
);

export const parseAccountRef = AccountRefParser;
export const parseAccountProfile = AccountProfileParser;
export const parseAccountCredentials = AccountCredentialsParser;
export const parseAccountSummary = AccountSummaryParser;
