import { Link } from "react-router-dom";
import type { Notification } from "@/domain/notification/notification";
import { NotificationModel } from "@/domain/notification/notification";
import { Status as StatusModel } from "@/domain/status/status";
import { formatRelativeTime } from "@/ui/lib/time";

type NotificationCardProps = Readonly<{
  notification: Notification;
}>;

export const NotificationCard = ({ notification }: NotificationCardProps) => {
  const status = notification.status;
  const body = status ? StatusModel.displayBody(status) : null;

  return (
    <article className="notification-card">
      <header className="notification-card-header">
        <Link to={`/profile/${notification.account.id}`} className="notification-actor">
          <img className="status-avatar" src={notification.account.avatar} alt="" loading="lazy" />
          <div>
            <p className="notification-summary">{NotificationModel.label(notification)}</p>
            <time className="app-muted" dateTime={notification.createdAt}>
              {formatRelativeTime(notification.createdAt)}
            </time>
          </div>
        </Link>
      </header>
      {status && body ? (
        <Link to={`/status/${body.id}`} className="notification-status-preview">
          {body.spoilerText ? <p className="app-muted">CW: {body.spoilerText}</p> : null}
          <div className="status-content" dangerouslySetInnerHTML={{ __html: body.content }} />
        </Link>
      ) : null}
    </article>
  );
};
