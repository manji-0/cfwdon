import { useCallback } from "react";
import { useNavigate } from "react-router-dom";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import { Status, type OriginalStatus } from "@/domain/status/status";
import { voteInPoll } from "@/infrastructure/api/poll";
import {
  blockAccount,
  muteAccount,
} from "@/infrastructure/api/relationship";
import { createReport } from "@/infrastructure/api/report";
import {
  bookmarkStatus,
  deleteStatus,
  favouriteStatus,
  pinStatus,
  reblogStatus,
  unbookmarkStatus,
  unfavouriteStatus,
  unpinStatus,
  unreblogStatus,
} from "@/infrastructure/api/status";

export type StatusActionHandlers = Readonly<{
  selfAccountId: string | null;
  onFavourite: (status: OriginalStatus) => void;
  onReblog: (status: OriginalStatus) => void;
  onBookmark: (status: OriginalStatus) => void;
  onReply: (status: OriginalStatus) => void;
  onDelete: (status: OriginalStatus) => void;
  onMute: (status: OriginalStatus) => void;
  onBlock: (status: OriginalStatus) => void;
  onReport: (status: OriginalStatus, comment: string) => void;
  onVotePoll: (status: OriginalStatus, choices: ReadonlyArray<number>) => void;
  onPin: (status: OriginalStatus) => void;
  onQuote: (status: OriginalStatus) => void;
  onEdit: (status: OriginalStatus) => void;
  onHistory: (status: OriginalStatus) => void;
}>;

export const useStatusActions = (options: {
  selfAccountId?: string | null;
  onReplace: (status: Status) => void;
  onRemove?: (statusId: string) => void;
  onError: (message: string) => void;
}): StatusActionHandlers => {
  const navigate = useNavigate();
  const selfAccountId = options.selfAccountId ?? null;
  const { onReplace, onRemove, onError } = options;

  const handleFavourite = useCallback(
    async (status: OriginalStatus) => {
      const result = status.favourited
        ? await unfavouriteStatus(status.id)
        : await favouriteStatus(status.id);
      if (result.isErr()) {
        onError(mastodonErrorMessage(result.error));
        return;
      }
      onReplace(result.value);
    },
    [onError, onReplace],
  );

  const handleReblog = useCallback(
    async (status: OriginalStatus) => {
      const result = status.reblogged
        ? await unreblogStatus(status.id)
        : await reblogStatus(status.id);
      if (result.isErr()) {
        onError(mastodonErrorMessage(result.error));
        return;
      }
      onReplace(result.value);
    },
    [onError, onReplace],
  );

  const handleBookmark = useCallback(
    async (status: OriginalStatus) => {
      const result = status.bookmarked
        ? await unbookmarkStatus(status.id)
        : await bookmarkStatus(status.id);
      if (result.isErr()) {
        onError(mastodonErrorMessage(result.error));
        return;
      }
      onReplace(result.value);
    },
    [onError, onReplace],
  );

  const handleDelete = useCallback(
    async (status: OriginalStatus) => {
      if (!window.confirm("この投稿を削除しますか？")) {
        return;
      }
      const result = await deleteStatus(status.id);
      if (result.isErr()) {
        onError(mastodonErrorMessage(result.error));
        return;
      }
      onRemove?.(status.id);
    },
    [onError, onRemove],
  );

  const handleMute = useCallback(
    async (status: OriginalStatus) => {
      if (!window.confirm(`@${status.account.acct} をミュートしますか？`)) {
        return;
      }
      const result = await muteAccount(status.account.id);
      if (result.isErr()) {
        onError(mastodonErrorMessage(result.error));
        return;
      }
      onRemove?.(status.id);
    },
    [onError, onRemove],
  );

  const handleBlock = useCallback(
    async (status: OriginalStatus) => {
      if (!window.confirm(`@${status.account.acct} をブロックしますか？`)) {
        return;
      }
      const result = await blockAccount(status.account.id);
      if (result.isErr()) {
        onError(mastodonErrorMessage(result.error));
        return;
      }
      onRemove?.(status.id);
    },
    [onError, onRemove],
  );

  const handleReport = useCallback(
    async (status: OriginalStatus, comment: string) => {
      const result = await createReport({
        accountId: status.account.id,
        statusIds: [status.id],
        comment,
      });
      if (result.isErr()) {
        onError(mastodonErrorMessage(result.error));
        return;
      }
      window.alert("通報を送信しました");
    },
    [onError],
  );

  const handleVotePoll = useCallback(
    async (status: OriginalStatus, choices: ReadonlyArray<number>) => {
      const poll = status.poll;
      if (!poll) {
        return;
      }
      const result = await voteInPoll(poll.id, choices);
      if (result.isErr()) {
        onError(mastodonErrorMessage(result.error));
        return;
      }
      onReplace(Status.withPoll(status, result.value));
    },
    [onError, onReplace],
  );

  const handlePin = useCallback(
    async (status: OriginalStatus) => {
      const result = status.pinned ? await unpinStatus(status.id) : await pinStatus(status.id);
      if (result.isErr()) {
        onError(mastodonErrorMessage(result.error));
        return;
      }
      onReplace(result.value);
    },
    [onError, onReplace],
  );

  return {
    selfAccountId,
    onFavourite: (status) => void handleFavourite(status),
    onReblog: (status) => void handleReblog(status),
    onBookmark: (status) => void handleBookmark(status),
    onReply: (status) => navigate(`/status/${status.id}`),
    onDelete: (status) => void handleDelete(status),
    onMute: (status) => void handleMute(status),
    onBlock: (status) => void handleBlock(status),
    onReport: (status, comment) => void handleReport(status, comment),
    onVotePoll: (status, choices) => void handleVotePoll(status, choices),
    onPin: (status) => void handlePin(status),
    onQuote: (status) => navigate(`/?quote=${encodeURIComponent(status.id)}`),
    onEdit: (status) => navigate(`/?edit=${encodeURIComponent(status.id)}`),
    onHistory: (status) => navigate(`/status/${status.id}/history`),
  };
};
