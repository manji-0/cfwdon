import { type ResultAsync } from "neverthrow";
import type { AccountRef } from "@/domain/account/account";
import type { AccountList } from "@/domain/lists/list";
import type { ListRepliesPolicy } from "@/domain/lists/replies-policy";
import type { Status } from "@/domain/status/status";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import {
  mastodonDeleteJson,
  mastodonFetchJson,
  mastodonPostJson,
  mastodonPutJson,
} from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import { parseAccountRefList } from "@/infrastructure/mastodon/parsers/account";
import { parseAccountList, parseAccountListCollection } from "@/infrastructure/mastodon/parsers/lists";
import { parseStatusList } from "@/infrastructure/mastodon/parsers/status";

export const fetchLists = (): ResultAsync<ReadonlyArray<AccountList>, MastodonFetchError> =>
  mastodonFetchJson("/api/v1/lists").andThen((raw) =>
    parseMastodon(parseAccountListCollection, raw),
  );

export type CreateListInput = Readonly<{
  title: string;
  repliesPolicy?: ListRepliesPolicy;
  exclusive?: boolean;
}>;

export const createList = (
  input: CreateListInput,
): ResultAsync<AccountList, MastodonFetchError> =>
  mastodonPostJson("/api/v1/lists", {
    title: input.title,
    replies_policy: input.repliesPolicy ?? "list",
    exclusive: input.exclusive ?? false,
  }).andThen((raw) => parseMastodon(parseAccountList, raw));

export type UpdateListInput = Readonly<{
  title: string;
  repliesPolicy?: ListRepliesPolicy;
  exclusive?: boolean;
}>;

export const updateList = (
  listId: string,
  input: UpdateListInput,
): ResultAsync<AccountList, MastodonFetchError> =>
  mastodonPutJson(`/api/v1/lists/${encodeURIComponent(listId)}`, {
    title: input.title,
    replies_policy: input.repliesPolicy ?? "list",
    exclusive: input.exclusive ?? false,
  }).andThen((raw) => parseMastodon(parseAccountList, raw));

export const deleteList = (listId: string): ResultAsync<null, MastodonFetchError> =>
  mastodonDeleteJson(`/api/v1/lists/${encodeURIComponent(listId)}`).map(() => null);

export const fetchListAccounts = (
  listId: string,
): ResultAsync<ReadonlyArray<AccountRef>, MastodonFetchError> =>
  mastodonFetchJson(`/api/v1/lists/${encodeURIComponent(listId)}/accounts`).andThen((raw) =>
    parseMastodon(parseAccountRefList, raw),
  );

export const addListAccounts = (
  listId: string,
  accountIds: ReadonlyArray<string>,
): ResultAsync<null, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/lists/${encodeURIComponent(listId)}/accounts`, {
    account_ids: [...accountIds],
  }).map(() => null);

export const removeListAccounts = (
  listId: string,
  accountIds: ReadonlyArray<string>,
): ResultAsync<null, MastodonFetchError> =>
  mastodonDeleteJson(`/api/v1/lists/${encodeURIComponent(listId)}/accounts`, {
    account_ids: [...accountIds],
  }).map(() => null);

export type ListTimelineQuery = Readonly<{
  maxId?: string;
  limit?: number;
}>;

export const fetchListTimeline = (
  listId: string,
  query: ListTimelineQuery = {},
): ResultAsync<ReadonlyArray<Status>, MastodonFetchError> => {
  const params = new URLSearchParams();
  params.set("limit", String(query.limit ?? 20));
  if (query.maxId) {
    params.set("max_id", query.maxId);
  }
  return mastodonFetchJson(
    `/api/v1/timelines/list/${encodeURIComponent(listId)}?${params}`,
  ).andThen((raw) => parseMastodon(parseStatusList, raw));
};
