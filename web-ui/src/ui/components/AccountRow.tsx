import { Link } from "react-router-dom";
import type { AccountProfile } from "@/domain/account/account";

type AccountRowProps = Readonly<{
  account: AccountProfile;
}>;

export const AccountRow = ({ account }: AccountRowProps) => (
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
);
