import { useAppNavigate } from "@/ui/hooks/useAppNavigate";
import { AppLink } from "@/ui/lib/app-link";
import { AppRoute } from "@/domain/navigation/route";
import { Notification } from "@/domain/notification/notification";
import { Status } from "@/domain/status/status";
import { LinkPreviewCard } from "@/ui/components/LinkPreviewCard";
import { StatusContent } from "@/ui/components/StatusContent";
import { formatRelativeTime } from "@/ui/lib/time";

type NotificationCardProps = Readonly<{
  notification: Notification;
  onAuthorizeFollow?: (accountId: string) => void;
  onRejectFollow?: (accountId: string) => void;
  onDismiss?: (notificationId: string) => void;
}>;

export const NotificationCard = ({
  notification,
  onAuthorizeFollow,
  onRejectFollow,
  onDismiss,
}: NotificationCardProps) => {
  const navigate = useAppNavigate();
  const status = Notification.status(notification);
  const body = status ? Status.displayBody(status) : null;
  const card = status ? Status.visibleCard(status) : null;
  const isFollowRequest = notification.kind === "FollowRequest";

  return (
    <article className="notification-card">
      <header className="notification-card-header">
        <AppLink to={AppRoute.account(notification.account.id)} className="notification-actor">
          <img className="status-avatar" src={notification.account.avatar} alt="" loading="lazy" />
          <div>
            <p className="notification-summary">{Notification.label(notification)}</p>
            <time className="app-muted" dateTime={notification.createdAt}>
              {formatRelativeTime(notification.createdAt)}
            </time>
          </div>
        </AppLink>
        {onDismiss ? (
          <button
            type="button"
            className="app-button app-button-secondary"
            onClick={() => onDismiss(notification.id)}
          >
            閉じる
          </button>
        ) : null}
      </header>
      {isFollowRequest ? (
        <div className="notification-follow-actions">
          <button
            type="button"
            className="app-button"
            onClick={() => onAuthorizeFollow?.(notification.account.id)}
          >
            承認
          </button>
          <button
            type="button"
            className="app-button app-button-secondary"
            onClick={() => onRejectFollow?.(notification.account.id)}
          >
            拒否
          </button>
        </div>
      ) : null}
      {status && body ? (
        <div
          className="notification-status-preview"
          role="link"
          tabIndex={0}
          onClick={(event) => {
            if ((event.target as HTMLElement).closest("a")) {
              return;
            }
            navigate(AppRoute.status(body.id));
          }}
          onKeyDown={(event) => {
            if (event.key !== "Enter" && event.key !== " ") {
              return;
            }
            if ((event.target as HTMLElement).closest("a")) {
              return;
            }
            event.preventDefault();
            navigate(AppRoute.status(body.id));
          }}
        >
          {body.spoilerText ? <p className="app-muted">CW: {body.spoilerText}</p> : null}
          <StatusContent html={body.content} />
          {card ? <LinkPreviewCard card={card} /> : null}
        </div>
      ) : null}
    </article>
  );
};
