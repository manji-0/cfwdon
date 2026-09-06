import { Link } from "react-router";
import { Status } from "@/domain/status/status";
import { LinkPreviewCard } from "@/ui/components/LinkPreviewCard";
import { StatusContent } from "@/ui/components/StatusContent";
import { formatRelativeTime } from "@/ui/lib/time";

type ChatMessageProps = Readonly<{
  status: Status;
  isOwn: boolean;
}>;

export const ChatMessage = ({ status, isOwn }: ChatMessageProps) => {
  const body = Status.displayBody(status);
  const card = Status.visibleCard(status);

  return (
    <article className={`chat-row${isOwn ? " is-own" : ""}`}>
      {isOwn ? null : (
        <Link className="chat-author" to={`/profile/${body.account.id}`}>
          <img className="status-avatar" src={body.account.avatar} alt="" loading="lazy" />
        </Link>
      )}
      <div className="chat-bubble">
        <div className="chat-meta">
          {isOwn ? null : (
            <Link className="status-display-name" to={`/profile/${body.account.id}`}>
              {body.account.displayName || body.account.username}
            </Link>
          )}
          <span className="app-muted">{formatRelativeTime(body.createdAt)}</span>
        </div>
        {body.spoilerText ? <p className="chat-cw">CW: {body.spoilerText}</p> : null}
        <StatusContent html={body.content} />
        {card ? <LinkPreviewCard card={card} /> : null}
      </div>
    </article>
  );
};
