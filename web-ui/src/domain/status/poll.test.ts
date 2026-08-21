import { describe, expect, it } from "vitest";
import { Poll, PollDraft } from "@/domain/status/poll";

describe("PollDraft", () => {
  it("is ready when two options are filled", () => {
    const draft = PollDraft.setOption(
      PollDraft.setOption(PollDraft.empty(), 0, "yes"),
      1,
      "no",
    );
    expect(PollDraft.isReady(draft)).toBe(true);
  });

  it("rejects a single filled option", () => {
    expect(PollDraft.isReady(PollDraft.setOption(PollDraft.empty(), 0, "yes"))).toBe(false);
  });

  it("caps options at four", () => {
    let draft = PollDraft.empty();
    draft = PollDraft.addOption(draft);
    draft = PollDraft.addOption(draft);
    draft = PollDraft.addOption(draft);
    expect(draft.options).toHaveLength(4);
  });
});

describe("Poll", () => {
  const poll: Poll = {
    id: "p1",
    expiresAt: "2026-08-23T00:00:00.000Z",
    expired: false,
    multiple: false,
    votesCount: 10,
    votersCount: 10,
    voted: true,
    ownVotes: [1],
    options: [
      { title: "yes", votesCount: 3 },
      { title: "no", votesCount: 7 },
    ],
  };

  it("computes vote percent from totals", () => {
    expect(Poll.percent(poll, poll.options[1]!)).toBe(70);
  });

  it("blocks voting after a vote", () => {
    expect(Poll.canVote(poll)).toBe(false);
    expect(Poll.votedOption(poll, 1)).toBe(true);
  });
});
