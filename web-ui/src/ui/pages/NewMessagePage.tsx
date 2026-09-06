import { useMemo, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { AccountProfile } from "@/domain/account/account";
import { ensureDirectMentions } from "@/domain/conversations/mentions";
import { Visibility } from "@/domain/status/visibility";
import { findConversationByStatusId } from "@/infrastructure/api/conversations";
import { createStatus } from "@/infrastructure/api/status";
import { AccountSearchPicker } from "@/ui/components/AccountSearchPicker";
import { AppShell } from "@/ui/components/AppShell";
import { Composer, type ComposerSubmitInput } from "@/ui/components/Composer";
import { useSession } from "@/ui/context/SessionContext";

export const NewMessagePage = () => {
  const navigate = useNavigate();
  const { session } = useSession();
  const selfId = session.kind === "Authenticated" ? session.account.id : "";
  const [selected, setSelected] = useState<ReadonlyArray<AccountProfile>>([]);

  const excludeIds = useMemo(() => {
    const ids = new Set(selected.map((account) => account.id));
    if (selfId) {
      ids.add(selfId);
    }
    return ids;
  }, [selected, selfId]);

  const removeAccount = (accountId: string) => {
    setSelected((current) => current.filter((account) => account.id !== accountId));
  };

  const handleSend = async (input: ComposerSubmitInput) => {
    if (selected.length === 0) {
      throw new Error("送信先を選んでください");
    }
    const result = await createStatus({
      text: ensureDirectMentions(input.text, selected),
      visibility: Visibility.toApi(Visibility.direct()),
      spoilerText: input.spoilerText,
      sensitive: input.sensitive,
      language: input.language,
      mediaIds: input.mediaIds,
    });
    if (result.isErr()) {
      throw new Error(mastodonErrorMessage(result.error));
    }
    const conversation = await findConversationByStatusId(result.value.id);
    if (conversation.isOk()) {
      navigate(`/messages/${conversation.value.id}`);
      return;
    }
    navigate("/messages");
  };

  return (
    <AppShell title="新しいメッセージ">
      <p className="thread-back">
        <Link to="/messages">← メッセージに戻る</Link>
      </p>
      <AccountSearchPicker
        placeholder="アカウントを検索"
        excludeIds={excludeIds}
        onSelect={(account) => {
          setSelected((current) =>
            current.some((item) => item.id === account.id) ? current : [...current, account],
          );
        }}
      />
      {selected.length > 0 ? (
        <div className="dm-chips">
          {selected.map((account) => (
            <button
              key={account.id}
              type="button"
              className="dm-chip"
              onClick={() => removeAccount(account.id)}
            >
              {account.displayName || account.username} ×
            </button>
          ))}
        </div>
      ) : (
        <p className="app-muted">1人以上の送信先を選んでください。</p>
      )}
      <Composer
        placeholder="メッセージを書く"
        submitLabel="送信"
        initialVisibility={Visibility.direct()}
        lockVisibility
        allowPoll={false}
        disabled={selected.length === 0}
        onSubmit={handleSend}
      />
    </AppShell>
  );
};
