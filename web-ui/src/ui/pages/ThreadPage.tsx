import { useEffect, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import { Status, type OriginalStatus } from "@/domain/status/status";
import { Visibility } from "@/domain/status/visibility";
import {
  bookmarkStatus,
  createStatus,
  favouriteStatus,
  fetchStatus,
  fetchStatusContext,
  reblogStatus,
  unbookmarkStatus,
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
      const [next] = Status.replaceInList([status], updated);
      return next;
    };
    setFocus((current) => (current ? apply(current) : current));
    setAncestors((current) => Status.replaceInList(current, updated));
    setDescendants((current) => Status.replaceInList(current, updated));
  };

  const handleFavourite = async (status: OriginalStatus) => {
    const result = status.favourited
      ? await unfavouriteStatus(status.id)
      : await favouriteStatus(status.id);
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    replaceStatus(result.value);
  };

  const handleReblog = async (status: OriginalStatus) => {
    const result = status.reblogged
      ? await unreblogStatus(status.id)
      : await reblogStatus(status.id);
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    replaceStatus(result.value);
  };

  const handleBookmark = async (status: OriginalStatus) => {
    const result = status.bookmarked
      ? await unbookmarkStatus(status.id)
      : await bookmarkStatus(status.id);
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

  const focusBody = focus ? Status.displayBody(focus) : null;
  const isDirectThread = focusBody?.visibility.kind === "Direct";

  return (
    <AppShell title={isDirectThread ? "ダイレクトメッセージ" : "スレッド"}>
      <p className="thread-back">
        <Link to={isDirectThread ? "/messages" : "/"}>
          ← {isDirectThread ? "メッセージに戻る" : "ホームに戻る"}
        </Link>
      </p>
      {error ? <p className="app-error">{error}</p> : null}
      {loading ? <div className="app-status">読み込み中…</div> : null}
      {!loading && focus ? (
        <>
          {isDirectThread ? (
            <p className="app-muted thread-dm-hint">
              ダイレクト返信は相手にのみ届きます。会話画面は{" "}
              <Link to="/messages">メッセージ</Link> から開けます。
            </p>
          ) : null}
          <div className="timeline">
            {ancestors.map((status) => (
              <StatusCard
                key={status.id}
                status={status}
                compact
                onFavourite={(body) => void handleFavourite(body)}
                onReblog={(body) => void handleReblog(body)}
                onBookmark={(body) => void handleBookmark(body)}
                onReply={() => navigate(`/status/${Status.displayBody(status).id}`)}
              />
            ))}
            <StatusCard
              status={focus}
              onFavourite={(body) => void handleFavourite(body)}
              onReblog={(body) => void handleReblog(body)}
              onBookmark={(body) => void handleBookmark(body)}
            />
            {descendants.map((status) => (
              <StatusCard
                key={status.id}
                status={status}
                onFavourite={(body) => void handleFavourite(body)}
                onReblog={(body) => void handleReblog(body)}
                onBookmark={(body) => void handleBookmark(body)}
                onReply={() => navigate(`/status/${Status.displayBody(status).id}`)}
              />
            ))}
          </div>
          <Composer
            key={`${statusId}-${focusBody?.visibility.kind ?? "public"}`}
            ref={composerRef}
            placeholder={isDirectThread ? "ダイレクトメッセージを送信" : "返信を投稿"}
            submitLabel={isDirectThread ? "送信" : "返信"}
            initialVisibility={isDirectThread ? Visibility.direct() : Visibility.public()}
            lockVisibility={isDirectThread}
            inReplyToId={statusId}
            onSubmit={handleReply}
          />
        </>
      ) : null}
    </AppShell>
  );
};
