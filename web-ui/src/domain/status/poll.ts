export type PollOption = Readonly<{
  title: string;
  votesCount: number | null;
}>;

export type Poll = Readonly<{
  id: string;
  expiresAt: string;
  expired: boolean;
  multiple: boolean;
  votesCount: number;
  votersCount: number | null;
  voted: boolean;
  ownVotes: ReadonlyArray<number>;
  options: ReadonlyArray<PollOption>;
}>;

export type PollDraft = Readonly<{
  options: ReadonlyArray<string>;
  expiresIn: number;
  multiple: boolean;
}>;

const MIN_OPTIONS = 2;
const MAX_OPTIONS = 4;
const MIN_EXPIRES_IN = 300;

export const PollDraft = {
  minOptions: MIN_OPTIONS,
  maxOptions: MAX_OPTIONS,
  minExpiresIn: MIN_EXPIRES_IN,

  empty: (): PollDraft => ({
    options: ["", ""],
    expiresIn: 86_400,
    multiple: false,
  }),

  filledOptions: (draft: PollDraft): ReadonlyArray<string> =>
    draft.options.map((option) => option.trim()).filter((option) => option.length > 0),

  isReady: (draft: PollDraft): boolean => {
    const filled = PollDraft.filledOptions(draft);
    return (
      filled.length >= MIN_OPTIONS &&
      filled.length <= MAX_OPTIONS &&
      draft.expiresIn >= MIN_EXPIRES_IN
    );
  },

  setOption: (draft: PollDraft, index: number, value: string): PollDraft => ({
    ...draft,
    options: draft.options.map((option, current) => (current === index ? value : option)),
  }),

  addOption: (draft: PollDraft): PollDraft =>
    draft.options.length >= MAX_OPTIONS ? draft : { ...draft, options: [...draft.options, ""] },

  removeOption: (draft: PollDraft, index: number): PollDraft =>
    draft.options.length <= MIN_OPTIONS
      ? draft
      : { ...draft, options: draft.options.filter((_, current) => current !== index) },
} as const;

export const Poll = {
  votedOption: (poll: Poll, index: number): boolean => poll.ownVotes.includes(index),

  canVote: (poll: Poll): boolean => !poll.expired && !poll.voted,

  percent: (poll: Poll, option: PollOption): number => {
    if (poll.votesCount <= 0 || option.votesCount === null) {
      return 0;
    }
    return Math.round((option.votesCount / poll.votesCount) * 100);
  },
} as const;
