import { useEffect, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { Status } from "@/domain/status/status";
import { Status as StatusModel } from "@/domain/status/status";
import { Visibility } from "@/domain/status/visibility";
import {
  createStatus,
  favouriteStatus,
  fetchStatus,
  fetchStatusContext,
  reblogStatus,
  unfavouriteStatus,
  unreblogStatus,
} from "@/infrastructure/api/status";
import { AppShell } from "@/ui/components/AppShell";
import { Composer, type ComposerHandle } from "@/ui/components/Composer";
import { useKeyboardShortcuts } from "@/ui/hooks/useKeyboardShortcuts";
import { StatusCard } from "@/ui/components/StatusCard";

export const ThreadPage = () => {
  const composerRef = useRef<ComposerHandle>(null);
  const { statusId = "" } = useParams();
  const navigate = useNavigate();
  const [focus, setFocus] = useState<Status | null>(null);
  const [ancestors, setAncestors] = useState<ReadonlyArray<Status>>([]);
  const [descendants, setDescendants] = useState<ReadonlyArray<Status>>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!statusId) {
      return;
    }
    let active = true;
    setLoading(true);
    setError("");
    void Promise.all([fetchStatus(statusId), fetchStatusContext(statusId)])
      .then(([statusResult, contextResult]) => {
        if (!active) {
          return;
        }
        if (statusResult.isErr()) {
          throw new Error(mastodonErrorMessage(statusResult.error));
        }
        if (contextResult.isErr()) {
          throw new Error(mastodonErrorMessage(contextResult.error));
        }
        setFocus(statusResult.value);
        setAncestors(contextResult.value.ancestors);
        setDescendants(contextResult.value.descendants);
      })
      .catch((loadError) => {
        if (active) {
          setError(loadError instanceof Error ? loadError.message : "スレッドの読み込みに失敗しました");
        }
      })
      .finally(() => {
        if (active) {
          setLoading(false);
        }
      });
    return () => {
      active = false;
    };
  }, [statusId]);

  const replaceStatus = (updated: Status) => {
    const apply = (status: Status) => {
      const body = StatusModel.displayBody(status);
      if (body.id === updated.id) {
        return status.reblog ? { ...status, reblog: updated } : updated;
      }
      return status;
    };
    setFocus((current) => (current ? apply(current) : current));
    setAncestors((current) => current.map(apply));
    setDescendants((current) => current.map(apply));
  };

  const handleFavourite = async (status: Status) => {
    const result = status.favourited
      ? await unfavouriteStatus(status.id)
      : await favouriteStatus(status.id);
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    replaceStatus(result.value);
  };

  const handleReblog = async (status: Status) => {
    const result = status.reblogged
      ? await unreblogStatus(status.id)
      : await reblogStatus(status.id);
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    replaceStatus(result.value);
  };

  useKeyboardShortcuts([
    {
      key: "n",
      handler: () => composerRef.current?.focus(),
      when: () => Boolean(focus),
    },
  ]);

  const handleReply = async (input: {
    text: string;
    visibility: ReturnType<typeof Visibility.public>;
    spoilerText: string;
    sensitive: boolean;
    inReplyToId?: string;
    mediaIds: ReadonlyArray<string>;
  }) => {
    const result = await createStatus({
      text: input.text,
      visibility: Visibility.toApi(input.visibility),
      spoilerText: input.spoilerText,
      sensitive: input.sensitive,
      inReplyToId: statusId,
      mediaIds: input.mediaIds,
    });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    setDescendants((current) => [...current, result.value]);
  };

  return (
    <AppShell title="スレッド">
      <p className="thread-back">
        <Link to="/">← ホームに戻る</Link>
      </p>
      {error ? <p className="app-error">{error}</p> : null}
      {loading ? <div className="app-status">読み込み中…</div> : null}
      {!loading && focus ? (
        <>
          <div className="timeline">
            {ancestors.map((status) => (
              <StatusCard
                key={status.id}
                status={status}
                compact
                onFavourite={(body) => void handleFavourite(body)}
                onReblog={(body) => void handleReblog(body)}
                onReply={() => navigate(`/status/${StatusModel.displayBody(status).id}`)}
              />
            ))}
            <StatusCard
              status={focus}
              onFavourite={(body) => void handleFavourite(body)}
              onReblog={(body) => void handleReblog(body)}
            />
            {descendants.map((status) => (
              <StatusCard
                key={status.id}
                status={status}
                onFavourite={(body) => void handleFavourite(body)}
                onReblog={(body) => void handleReblog(body)}
                onReply={() => navigate(`/status/${StatusModel.displayBody(status).id}`)}
              />
            ))}
          </div>
          <Composer
            ref={composerRef}
            placeholder="返信を投稿"
            submitLabel="返信"
            inReplyToId={statusId}
            onSubmit={handleReply}
          />
        </>
      ) : null}
    </AppShell>
  );
};
