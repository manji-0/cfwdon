import { AppRoute } from "@/domain/navigation/route";
import { useAppPathname } from "@/ui/hooks/useAppPathname";
import { AppLink } from "@/ui/lib/app-link";
import {
  IconBell,
  IconBrand,
  IconHome,
  IconPen,
  IconSearch,
  IconUser,
} from "@/ui/components/icons";
import { useCompose } from "@/ui/context/ComposeContext";
import { useSession } from "@/ui/context/SessionContext";
import { useUnreadMessages } from "@/ui/context/UnreadMessagesContext";
import { useUnreadNotifications } from "@/ui/context/UnreadNotificationsContext";

const unreadLabel = (count: number): string => (count > 99 ? "99+" : String(count));

const useInboxUnread = (): number => {
  const { unreadCount: messages } = useUnreadMessages();
  const { unreadCount: notifications } = useUnreadNotifications();
  return messages + notifications;
};

const useNavIdentity = () => {
  const pathname = useAppPathname();
  const { session } = useSession();
  const selfAccountId = session.kind === "Authenticated" ? session.account.id : null;
  return {
    inboxActive: AppRoute.isInboxPath(pathname),
    meActive: AppRoute.isMePath(pathname, selfAccountId),
  };
};

const UnreadBadge = ({ count }: Readonly<{ count: number }>) =>
  count > 0 ? (
    <span className="nav-unread-badge" aria-hidden="true">
      {unreadLabel(count)}
    </span>
  ) : null;

export const SidebarNav = () => {
  const { openNew } = useCompose();
  const inboxUnread = useInboxUnread();
  const { inboxActive, meActive } = useNavIdentity();

  return (
    <nav className="app-nav" aria-label="メイン">
      <AppLink
        to={AppRoute.home()}
        end
        className="app-brand"
        aria-label="cfwdon ホーム"
      >
        <span className="app-brand-mark" aria-hidden="true">
          <IconBrand />
        </span>
      </AppLink>
      <AppLink
        to={AppRoute.home()}
        end
        className={({ isActive }) => `app-nav-link${isActive ? " is-active" : ""}`}
        aria-label="ホーム"
      >
        <IconHome aria-hidden="true" />
      </AppLink>
      <AppLink
        to={AppRoute.notifications()}
        className={`app-nav-link${inboxActive ? " is-active" : ""}`}
        aria-label={inboxUnread > 0 ? `受信（未読 ${inboxUnread}）` : "受信"}
      >
        <IconBell aria-hidden="true" />
        <UnreadBadge count={inboxUnread} />
      </AppLink>
      <AppLink
        to={AppRoute.search()}
        className={({ isActive }) => `app-nav-link${isActive ? " is-active" : ""}`}
        aria-label="検索"
      >
        <IconSearch aria-hidden="true" />
      </AppLink>
      <AppLink
        to={AppRoute.profile()}
        className={`app-nav-link${meActive ? " is-active" : ""}`}
        aria-label="自分"
      >
        <IconUser aria-hidden="true" />
      </AppLink>
      <button
        type="button"
        className="app-nav-link app-nav-compose"
        aria-label="投稿"
        onClick={() => openNew()}
      >
        <IconPen aria-hidden="true" />
      </button>
    </nav>
  );
};

export const BottomNav = () => {
  const { openNew } = useCompose();
  const inboxUnread = useInboxUnread();
  const { inboxActive, meActive } = useNavIdentity();

  return (
    <nav className="app-bottom-nav" aria-label="モバイルナビ">
      <AppLink
        to={AppRoute.home()}
        end
        className={({ isActive }) => (isActive ? "is-active" : undefined)}
        aria-label="ホーム"
      >
        <IconHome aria-hidden="true" />
        <span>ホーム</span>
      </AppLink>
      <AppLink
        to={AppRoute.notifications()}
        className={inboxActive ? "is-active" : undefined}
        aria-label={inboxUnread > 0 ? `受信（未読 ${inboxUnread}）` : "受信"}
      >
        <IconBell aria-hidden="true" />
        <span>受信</span>
        <UnreadBadge count={inboxUnread} />
      </AppLink>
      <button type="button" aria-label="投稿" onClick={() => openNew()}>
        <IconPen aria-hidden="true" />
        <span>投稿</span>
      </button>
      <AppLink
        to={AppRoute.search()}
        className={({ isActive }) => (isActive ? "is-active" : undefined)}
        aria-label="検索"
      >
        <IconSearch aria-hidden="true" />
        <span>検索</span>
      </AppLink>
      <AppLink
        to={AppRoute.profile()}
        className={meActive ? "is-active" : undefined}
        aria-label="自分"
      >
        <IconUser aria-hidden="true" />
        <span>自分</span>
      </AppLink>
    </nav>
  );
};
