import { type } from "arktype";
import { type ResultAsync } from "neverthrow";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import {
  mastodonDeleteJson,
  mastodonFetchJson,
  mastodonPostJson,
} from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";

const DomainListParser = type("string[]");

export const fetchDomainBlocks = (): ResultAsync<ReadonlyArray<string>, MastodonFetchError> =>
  mastodonFetchJson("/api/v1/domain_blocks").andThen((raw) =>
    parseMastodon(DomainListParser, raw),
  );

export const blockDomain = (domain: string): ResultAsync<void, MastodonFetchError> =>
  mastodonPostJson("/api/v1/domain_blocks", { domain }).map(() => undefined);

export const unblockDomain = (domain: string): ResultAsync<void, MastodonFetchError> =>
  mastodonDeleteJson("/api/v1/domain_blocks", { domain }).map(() => undefined);
