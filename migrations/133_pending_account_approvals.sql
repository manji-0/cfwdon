-- Mastodon `User#approved = false`: a sign-up held for an admin to approve or
-- reject. An account without a row here is approved. `reason` is the sign-up
-- `reason` (Mastodon's `invite_request` text).
CREATE TABLE IF NOT EXISTS pending_account_approvals (
    account_id TEXT PRIMARY KEY,
    reason TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_pending_account_approvals_created_at
    ON pending_account_approvals (created_at, account_id);
