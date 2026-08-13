import { Link } from "react-router-dom";
import type { Status } from "@/domain/status/status";
import { formatRelativeTime } from "@/ui/lib/time";

type ChatMessageProps = Readonly<{
  status: Status;
  isOwn: boolean;
}>;

export const ChatMessage = ({ status, isOwn }: ChatMessageProps) => (
  <article className={`chat-row${isOwn ? " is-own" : ""}`}>
    {isOwn ? null : (
      <Link className="chat-author" to={`/profile/${status.account.id}`}>
        <img className="status-avatar" src={status.account.avatar} alt="" loading="lazy" />
      </Link>
    )}
    <div className="chat-bubble">
      <div className="chat-meta">
        {isOwn ? null : (
          <Link className="status-display-name" to={`/profile/${status.account.id}`}>
            {status.account.displayName || status.account.username}
          </Link>
        )}
        <span className="app-muted">{formatRelativeTime(status.createdAt)}</span>
      </div>
      {status.spoilerText ? <p className="chat-cw">CW: {status.spoilerText}</p> : null}
      <div className="status-content" dangerouslySetInnerHTML={{ __html: status.content }} />
    </div>
  </article>
);
