import { useEffect, useMemo, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import { Conversation } from "@/domain/conversations/conversation";
import { ensureDirectMentions } from "@/domain/conversations/mentions";
import { conversationAcctsLabel, conversationTitle } from "@/domain/conversations/participants";
import {
  appendConversationStatus,
  flattenConversationStatuses,
} from "@/domain/conversations/thread";
import type { Status } from "@/domain/status/status";
import { Visibility } from "@/domain/status/visibility";
import {
  deleteConversation,
  findConversationById,
  markConversationRead,
} from "@/infrastructure/api/conversations";
import { createStatus, fetchStatus, fetchStatusContext } from "@/infrastructure/api/status";
import { StreamingUser } from "@/infrastructure/streaming/mastodon-stream";
import { AppShell } from "@/ui/components/AppShell";
import { ChatMessage } from "@/ui/components/ChatMessage";
import { Composer, type ComposerSubmitInput } from "@/ui/components/Composer";
import { useSession } from "@/ui/context/SessionContext";
import { useUnreadMessages } from "@/ui/context/UnreadMessagesContext";

export const ConversationPage = () => {
  const { conversationId = "" } = useParams();
  const navigate = useNavigate();
  const { session } = useSession();
  const { refreshUnreadCount } = useUnreadMessages();
  const [conversation, setConversation] = useState<Conversation | null>(null);
  const [statuses, setStatuses] = useState<ReadonlyArray<Status>>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  const selfId = session.kind === "Authenticated" ? session.account.id : "";

  useEffect(() => {
    if (!conversationId) {
      return;
    }
    let active = true;
    setLoading(true);
    setError("");
    void (async () => {
      const found = await findConversationById(conversationId);
      if (!active) {
        return;
      }
      if (found.isErr()) {
        throw new Error(mastodonErrorMessage(found.error));
      }
      setConversation(found.value);
      if (Conversation.isUnread(found.value)) {
        const read = await markConversationRead(conversationId);
        if (read.isOk() && active) {
          setConversation(read.value);
          refreshUnreadCount();
        }
      }
      if (!found.value.lastStatus) {
        setStatuses([]);
        return;
      }
      const [statusResult, contextResult] = await Promise.all([
        fetchStatus(found.value.lastStatus.id),
        fetchStatusContext(found.value.lastStatus.id),
      ]);
      if (!active) {
        return;
      }
      if (statusResult.isErr()) {
        throw new Error(mastodonErrorMessage(statusResult.error));
      }
      if (contextResult.isErr()) {
        throw new Error(mastodonErrorMessage(contextResult.error));
      }
      setStatuses(
        flattenConversationStatuses(
          contextResult.value.ancestors,
          statusResult.value,
          contextResult.value.descendants,
        ),
      );
    })()
      .catch((loadError) => {
        if (active) {
          setError(loadError instanceof Error ? loadError.message : "会話の読み込みに失敗しました");
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
  }, [conversationId, refreshUnreadCount]);

  useEffect(() => {
    const subscription = StreamingUser.subscribe((event) => {
      if (event.kind === "Conversation" && event.conversation.id === conversationId) {
        setConversation(event.conversation);
        return;
      }
      if (event.kind !== "Update") {
        return;
      }
      setStatuses((current) => appendConversationStatus(current, event.status));
    });
    return () => subscription.close();
  }, [conversationId]);

  const participants = conversation?.accounts ?? [];
  const mentionTargets = useMemo(
    () => participants.filter((account) => account.id !== selfId),
    [participants, selfId],
  );
  const latestStatusId = statuses.at(-1)?.id ?? conversation?.lastStatus?.id;

  const handleSend = async (input: ComposerSubmitInput) => {
    const result = await createStatus({
      text: ensureDirectMentions(input.text, mentionTargets),
      visibility: Visibility.toApi(Visibility.direct()),
      spoilerText: input.spoilerText,
      sensitive: input.sensitive,
      language: input.language,
      inReplyToId: latestStatusId,
      mediaIds: input.mediaIds,
    });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    setStatuses((current) => appendConversationStatus(current, result.value));
  };

  const handleDelete = async () => {
    if (!conversationId || !window.confirm("この会話を削除しますか？")) {
      return;
    }
    const result = await deleteConversation(conversationId);
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      return;
    }
    navigate("/messages");
  };

  return (
    <AppShell title={conversation ? conversationTitle(conversation.accounts) : "メッセージ"}>
      <p className="thread-back">
        <Link to="/messages">← メッセージに戻る</Link>
      </p>
      {error ? <p className="app-error">{error}</p> : null}
      {loading ? <div className="app-status">読み込み中…</div> : null}
      {!loading && conversation ? (
        <>
          <div className="conversation-header-card app-card">
            <div>
              <p className="conversation-participants">{conversationTitle(conversation.accounts)}</p>
              <p className="app-muted">{conversationAcctsLabel(conversation.accounts) || "参加者なし"}</p>
            </div>
            <button
              type="button"
              className="app-button app-button-secondary"
              onClick={() => void handleDelete()}
            >
              削除
            </button>
          </div>
          {statuses.length === 0 ? (
            <p className="app-muted">まだメッセージはありません。最初のメッセージを送信できます。</p>
          ) : (
            <div className="chat-thread">
              {statuses.map((status) => (
                <ChatMessage key={status.id} status={status} isOwn={status.account.id === selfId} />
              ))}
            </div>
          )}
          <Composer
            placeholder="メッセージを送信"
            submitLabel="送信"
            initialVisibility={Visibility.direct()}
            lockVisibility
            allowPoll={false}
            inReplyToId={latestStatusId}
            onSubmit={handleSend}
          />
        </>
      ) : null}
    </AppShell>
  );
};
