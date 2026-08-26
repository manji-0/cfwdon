import { useState } from "react";
import { Poll, type Poll as PollValue } from "@/domain/status/poll";

type PollCardProps = Readonly<{
  poll: PollValue;
  disabled?: boolean;
  onVote?: (choices: ReadonlyArray<number>) => void;
}>;

export const PollCard = ({ poll, disabled = false, onVote }: PollCardProps) => {
  const [selected, setSelected] = useState<ReadonlyArray<number>>(poll.ownVotes);
  const showResults = poll.voted || poll.expired || !onVote;
  const canSubmit = Poll.canVote(poll) && selected.length > 0 && !disabled;

  const toggleChoice = (index: number) => {
    if (!Poll.canVote(poll) || disabled) {
      return;
    }
    if (poll.multiple) {
      setSelected((current) =>
        current.includes(index) ? current.filter((choice) => choice !== index) : [...current, index],
      );
      return;
    }
    setSelected([index]);
  };

  return (
    <div className="poll-card">
      <ul className="poll-options">
        {poll.options.map((option, index) => {
          const percent = Poll.percent(poll, option);
          const votedHere = Poll.votedOption(poll, index) || selected.includes(index);
          return (
            <li key={`${poll.id}-${index}`}>
              {showResults ? (
                <div className={`poll-result${votedHere ? " is-voted" : ""}`}>
                  <span className="poll-result-fill" style={{ width: `${percent}%` }} />
                  <span className="poll-result-label">
                    {option.title}
                    {option.votesCount !== null ? ` · ${percent}%` : ""}
                  </span>
                </div>
              ) : (
                <label className="poll-choice">
                  <input
                    type={poll.multiple ? "checkbox" : "radio"}
                    name={`poll-${poll.id}`}
                    checked={selected.includes(index)}
                    onChange={() => toggleChoice(index)}
                    disabled={disabled}
                  />
                  <span>{option.title}</span>
                </label>
              )}
            </li>
          );
        })}
      </ul>
      <p className="app-muted poll-meta">
        {poll.votesCount} 票
        {poll.expired ? " · 終了" : ""}
        {poll.multiple ? " · 複数選択可" : ""}
      </p>
      {Poll.canVote(poll) && onVote ? (
        <button
          type="button"
          className="app-button app-button-secondary"
          disabled={!canSubmit}
          onClick={() => onVote(selected)}
        >
          投票する
        </button>
      ) : null}
    </div>
  );
};
