import { Link } from "react-router";
import type { AccountProfile } from "@/domain/account/account";

type AccountRowProps = Readonly<{
  account: AccountProfile;
  actionLabel?: string;
  onAction?: () => void;
  actionDisabled?: boolean;
}>;

export const AccountRow = ({
  account,
  actionLabel,
  onAction,
  actionDisabled = false,
}: AccountRowProps) => (
  <div className="account-row-wrap">
    <Link className="account-row" to={`/profile/${account.id}`}>
      <img className="status-avatar" src={account.avatar} alt="" loading="lazy" />
      <div className="account-row-meta">
        <span className="status-display-name">{account.displayName || account.username}</span>
        <span className="status-acct">@{account.acct}</span>
        {account.note ? (
          <span
            className="account-row-note app-muted"
            dangerouslySetInnerHTML={{ __html: account.note }}
          />
        ) : null}
      </div>
    </Link>
    {onAction && actionLabel ? (
      <button
        type="button"
        className="app-button app-button-secondary account-row-action"
        disabled={actionDisabled}
        onClick={onAction}
      >
        {actionLabel}
      </button>
    ) : null}
  </div>
);
