import { type ResultAsync } from "neverthrow";
import type { AccountRef } from "@/domain/account/account";
import type { MastodonFetchError } from "@/domain/errors/http-error";
import type { PageQuery } from "@/domain/pagination";
import { mastodonFetchJson, pageParams } from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import { parseAccountList } from "@/infrastructure/mastodon/parsers/moderation";

export const fetchMutedAccounts = (
  query: PageQuery = {},
): ResultAsync<ReadonlyArray<AccountRef>, MastodonFetchError> =>
  mastodonFetchJson(`/api/v1/mutes?${pageParams(query)}`).andThen((raw) =>
    parseMastodon(parseAccountList, raw),
  );

export const fetchBlockedAccounts = (
  query: PageQuery = {},
): ResultAsync<ReadonlyArray<AccountRef>, MastodonFetchError> =>
  mastodonFetchJson(`/api/v1/blocks?${pageParams(query)}`).andThen((raw) =>
    parseMastodon(parseAccountList, raw),
  );
