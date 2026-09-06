import { useEffect, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import { Status } from "@/domain/status/status";
import { Visibility } from "@/domain/status/visibility";
import { createStatus, fetchStatus, fetchStatusContext } from "@/infrastructure/api/status";
import { AppShell } from "@/ui/components/AppShell";
import { Composer, type ComposerHandle, type ComposerSubmitInput } from "@/ui/components/Composer";
import { useKeyboardShortcuts } from "@/ui/hooks/useKeyboardShortcuts";
import { useStatusActions } from "@/ui/hooks/useStatusActions";
import { StatusCard } from "@/ui/components/StatusCard";
import { useSession } from "@/ui/context/SessionContext";

export const ThreadPage = () => {
  const composerRef = useRef<ComposerHandle>(null);
  const { statusId = "" } = useParams();
  const navigate = useNavigate();
  const { session } = useSession();
  const selfAccountId = session.kind === "Authenticated" ? session.account.id : null;
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

  const actions = useStatusActions({
    selfAccountId,
    onReplace: replaceStatus,
    onRemove: (removedId) => {
      if (removedId === statusId) {
        navigate("/");
        return;
      }
      setAncestors((current) => Status.removeById(current, removedId));
      setDescendants((current) => Status.removeById(current, removedId));
    },
    onError: setError,
  });

  useKeyboardShortcuts([
    {
      key: "n",
      handler: () => composerRef.current?.focus(),
      when: () => Boolean(focus),
    },
  ]);

  const handleReply = async (input: ComposerSubmitInput) => {
    const result = await createStatus({
      text: input.text,
      visibility: Visibility.toApi(input.visibility),
      spoilerText: input.spoilerText,
      sensitive: input.sensitive,
      language: input.language,
      inReplyToId: statusId,
      mediaIds: input.mediaIds,
      poll: input.poll,
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
              <StatusCard key={status.id} status={status} compact {...actions} />
            ))}
            <StatusCard status={focus} {...actions} />
            {descendants.map((status) => (
              <StatusCard key={status.id} status={status} {...actions} />
            ))}
          </div>
          <Composer
            key={`${statusId}-${focusBody?.visibility.kind ?? "public"}`}
            ref={composerRef}
            placeholder={isDirectThread ? "ダイレクトメッセージを送信" : "返信を投稿"}
            submitLabel={isDirectThread ? "送信" : "返信"}
            initialVisibility={isDirectThread ? Visibility.direct() : Visibility.public()}
            lockVisibility={isDirectThread}
            applyPostingDefaults={!isDirectThread}
            allowPoll={!isDirectThread}
            inReplyToId={statusId}
            onSubmit={handleReply}
          />
        </>
      ) : null}
    </AppShell>
  );
};
