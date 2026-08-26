import type { Poll } from "@/domain/status/poll";
import { mastodon } from "@/infrastructure/mastodon/parsers/definitions";

export const parsePoll = mastodon.type("PollApi").pipe(
  (value): Poll => ({
    id: value.id,
    expiresAt: value.expires_at ?? "",
    expired: value.expired,
    multiple: value.multiple,
    votesCount: value.votes_count,
    votersCount: value.voters_count ?? null,
    voted: value.voted ?? false,
    ownVotes: value.own_votes ?? [],
    options: value.options.map((option) => ({
      title: option.title,
      votesCount: option.votes_count ?? null,
    })),
  }),
);
