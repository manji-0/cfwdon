import { Link, useNavigate } from "react-router-dom";
import type { Notification as NotificationState } from "@/domain/notification/notification";
import { Notification } from "@/domain/notification/notification";
import { PreviewCard } from "@/domain/status/preview-card";
import { Status as StatusModel } from "@/domain/status/status";
import { LinkPreviewCard } from "@/ui/components/LinkPreviewCard";
import { StatusContent } from "@/ui/components/StatusContent";
import { formatRelativeTime } from "@/ui/lib/time";

type NotificationCardProps = Readonly<{
  notification: NotificationState;
}>;

export const NotificationCard = ({ notification }: NotificationCardProps) => {
  const navigate = useNavigate();
  const status = Notification.status(notification);
  const body = status ? StatusModel.displayBody(status) : null;

  return (
    <article className="notification-card">
      <header className="notification-card-header">
        <Link to={`/profile/${notification.account.id}`} className="notification-actor">
          <img className="status-avatar" src={notification.account.avatar} alt="" loading="lazy" />
          <div>
            <p className="notification-summary">{Notification.label(notification)}</p>
            <time className="app-muted" dateTime={notification.createdAt}>
              {formatRelativeTime(notification.createdAt)}
            </time>
          </div>
        </Link>
      </header>
      {status && body ? (
        <div
          className="notification-status-preview"
          role="link"
          tabIndex={0}
          onClick={(event) => {
            if ((event.target as HTMLElement).closest("a")) {
              return;
            }
            navigate(`/status/${body.id}`);
          }}
          onKeyDown={(event) => {
            if (event.key !== "Enter" && event.key !== " ") {
              return;
            }
            if ((event.target as HTMLElement).closest("a")) {
              return;
            }
            event.preventDefault();
            navigate(`/status/${body.id}`);
          }}
        >
          {body.spoilerText ? <p className="app-muted">CW: {body.spoilerText}</p> : null}
          <StatusContent html={body.content} />
          {PreviewCard.isVisible(body) ? <LinkPreviewCard card={body.card!} /> : null}
        </div>
      ) : null}
    </article>
  );
};
