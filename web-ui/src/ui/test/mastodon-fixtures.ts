import type { AccountProfile } from "@/domain/account/account";

export const credentialsApi = {
  id: "acct-1",
  display_name: "Alice",
  note: "<p>hi</p>",
  avatar: "https://example.test/a.png",
  header: "",
  username: "alice",
  acct: "alice",
  locked: false,
  bot: false,
  discoverable: true,
  fields: [],
  source: {
    note: "hi",
    privacy: "public",
    sensitive: false,
    language: "ja",
    quote_policy: "public",
  },
} as const;

export const preferencesApi = {
  "posting:default:visibility": "public",
  "posting:default:sensitive": false,
  "posting:default:language": "ja",
  "posting:default:quote_policy": "public",
  "reading:expand:media": "default",
  "reading:expand:spoilers": false,
} as const;

export const notificationPolicyApi = {
  for_not_following: "accept",
  for_not_followers: "accept",
  for_new_accounts: "accept",
  for_private_mentions: "accept",
  for_limited_accounts: "accept",
} as const;

export const keywordFilterApi = {
  id: "filter-1",
  title: "ads",
  context: ["home"],
  expires_at: "2020-01-01T00:00:00.000Z",
  filter_action: "warn",
  keywords: [{ id: "kw-1", keyword: "spam", whole_word: false }],
  statuses: [],
} as const;

export const featuredTagApi = {
  id: "tag-1",
  name: "cfwdon",
  url: "https://example.test/tags/cfwdon",
  statuses_count: 4,
  last_status_at: "2026-09-01T00:00:00.000Z",
} as const;

export const accountProfileApi = {
  id: "acct-1",
  username: "alice",
  acct: "alice",
  display_name: "Alice",
  avatar: "https://example.test/a.png",
  header: "",
  note: "<p>hi</p>",
  followers_count: 1,
  following_count: 2,
  statuses_count: 3,
  locked: false,
  bot: false,
  discoverable: true,
  fields: [{ name: "site", value: "https://example.test", verified_at: null }],
} as const satisfies Record<string, unknown>;

export const accountProfileFixture = {
  id: "acct-1",
  username: "alice",
  acct: "alice",
  displayName: "Alice",
  avatar: "https://example.test/a.png",
  header: "",
  note: "<p>hi</p>",
  followersCount: 1,
  followingCount: 2,
  statusesCount: 3,
  locked: false,
  bot: false,
  discoverable: true,
  fields: [{ name: "site", value: "https://example.test", verifiedAt: null }],
} as const satisfies AccountProfile;

export const accountListApi = {
  id: "list-1",
  title: "Friends",
  replies_policy: "list",
  exclusive: false,
} as const;
