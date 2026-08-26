import { type ResultAsync } from "neverthrow";
import type { Poll } from "@/domain/status/poll";
import type { MastodonFetchError } from "@/infrastructure/http/mastodon-fetch";
import { mastodonPostJson } from "@/infrastructure/http/mastodon-fetch";
import { parseMastodon } from "@/infrastructure/mastodon/parse";
import { parsePoll } from "@/infrastructure/mastodon/parsers/poll";

export const voteInPoll = (
  pollId: string,
  choices: ReadonlyArray<number>,
): ResultAsync<Poll, MastodonFetchError> =>
  mastodonPostJson(`/api/v1/polls/${encodeURIComponent(pollId)}/votes`, {
    choices,
  }).andThen((raw) => parseMastodon(parsePoll, raw));
