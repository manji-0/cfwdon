import { type ResultAsync } from "neverthrow";
import type { AccountRef } from "@/domain/account/account";
import type { MastodonFetchError } from "@/domain/errors/http-error";
import type { Page, PageQuery } from "@/domain/pagination";
import { mastodonFetchPage, pageParams } from "@/infrastructure/http/mastodon-fetch";
import { parseAccountList } from "@/infrastructure/mastodon/parsers/moderation";

export const fetchMutedAccounts = (
  query: PageQuery = {},
): ResultAsync<Page<AccountRef>, MastodonFetchError> =>
  mastodonFetchPage(`/api/v1/mutes?${pageParams(query)}`, parseAccountList);

export const fetchBlockedAccounts = (
  query: PageQuery = {},
): ResultAsync<Page<AccountRef>, MastodonFetchError> =>
  mastodonFetchPage(`/api/v1/blocks?${pageParams(query)}`, parseAccountList);
