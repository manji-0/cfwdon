//! Accounts held for an admin's approval (`pending_account_approvals`). An
//! account without a row is approved.

use crate::db_utils::d1_results;
use crate::oauth_store::CredentialAccount;
use crate::tracked_d1::{D1Database, D1PreparedStatement};
use cfwdon_domain::{AccountApproval, LocalAccount, LocalAccountRecord};
use serde::Deserialize;
use worker::Result;
use worker::d1::D1Type;

/// A pending account as an admin reviews it.
#[derive(Debug, Clone)]
pub(crate) struct PendingAccount {
    pub(crate) account: LocalAccount,
    /// What it said when it signed up (Mastodon's `invite_request`).
    pub(crate) reason: Option<String>,
    /// No e-mail confirmation is outstanding.
    pub(crate) confirmed: bool,
}

#[derive(Deserialize)]
struct PendingAccountRow {
    #[serde(flatten)]
    account: LocalAccountRecord,
    reason: Option<String>,
    confirmed: i64,
}

impl From<PendingAccountRow> for PendingAccount {
    fn from(row: PendingAccountRow) -> Self {
        Self {
            account: LocalAccount::from_record(row.account),
            reason: row.reason,
            confirmed: row.confirmed != 0,
        }
    }
}

const PENDING_ACCOUNT_SELECT: &str =
    "SELECT a.id, a.username, a.access_email, a.display_name, a.bio_html, a.bio_text,
            a.fields_json, a.locked, a.bot, a.discoverable, a.default_post_visibility,
            a.default_quote_policy, a.default_sensitive, a.default_language,
            a.avatar_object_key, a.avatar_content_type, a.header_object_key,
            a.header_content_type, '' AS private_key_jwk, a.public_key_pem,
            a.created_at, p.reason,
            NOT EXISTS (
                SELECT 1 FROM pending_email_confirmations e WHERE e.account_id = a.id
            ) AS confirmed
     FROM pending_account_approvals p
     INNER JOIN accounts a ON a.id = p.account_id";

/// Hold `account_id` for approval.
pub(crate) fn hold_account_for_approval_statement(
    db: &D1Database,
    account_id: &str,
    reason: Option<&str>,
) -> Result<D1PreparedStatement> {
    let bindings = [
        D1Type::Text(account_id),
        reason.map_or(D1Type::Null, D1Type::Text),
    ];
    db.prepare(
        "INSERT INTO pending_account_approvals (account_id, reason)
         VALUES (?1, ?2)
         ON CONFLICT(account_id) DO NOTHING",
    )
    .bind_refs(bindings.iter())
}

/// `account_id` while it waits for approval; `None` once approved.
pub(crate) async fn find_pending_account(
    db: &D1Database,
    account_id: &str,
) -> Result<Option<PendingAccount>> {
    Ok(db
        .prepare(format!("{PENDING_ACCOUNT_SELECT} WHERE p.account_id = ?1"))
        .bind_refs(&D1Type::Text(account_id))?
        .first::<PendingAccountRow>(None)
        .await?
        .map(PendingAccount::from))
}

/// The account an e-mail signs in as (an Auth0 identity or a password
/// login), with its approval, in one read.
pub(crate) async fn find_credential_account_by_email(
    db: &D1Database,
    email: &str,
) -> Result<Option<CredentialAccount>> {
    #[derive(Deserialize)]
    struct Row {
        #[serde(flatten)]
        account: LocalAccountRecord,
        pending_account_id: Option<String>,
    }
    let row = db
        .prepare(
            "SELECT a.id, a.username, a.access_email, a.display_name, a.bio_html, a.bio_text,
                    a.fields_json, a.locked, a.bot, a.discoverable, a.default_post_visibility,
                    a.default_quote_policy, a.default_sensitive, a.default_language,
                    a.avatar_object_key, a.avatar_content_type, a.header_object_key,
                    a.header_content_type, '' AS private_key_jwk, a.public_key_pem,
                    a.created_at, p.account_id AS pending_account_id
             FROM accounts a
             LEFT JOIN pending_account_approvals p ON p.account_id = a.id
             WHERE a.access_email = ?1
             LIMIT 1",
        )
        .bind_refs(&D1Type::Text(email))?
        .first::<Row>(None)
        .await?;
    Ok(row.map(|row| CredentialAccount {
        account: LocalAccount::from_record(row.account),
        approval: if row.pending_account_id.is_some() {
            AccountApproval::Pending
        } else {
            AccountApproval::Approved
        },
    }))
}

/// The approved account holding `email`, for lookups that are not this
/// request's credential (admin notification recipients).
pub(crate) async fn find_approved_account_by_email(
    db: &D1Database,
    email: &str,
) -> Result<Option<LocalAccount>> {
    Ok(find_credential_account_by_email(db, email)
        .await?
        .filter(|credential| credential.approval == AccountApproval::Approved)
        .map(|credential| credential.account))
}

/// The oldest pending accounts first, as Mastodon's admin queue lists them.
pub(crate) async fn list_pending_accounts(
    db: &D1Database,
    limit: u32,
) -> Result<Vec<PendingAccount>> {
    let limit = i32::try_from(limit).unwrap_or(i32::MAX);
    let result = db
        .prepare(format!(
            "{PENDING_ACCOUNT_SELECT} ORDER BY p.created_at ASC, p.account_id ASC LIMIT ?1"
        ))
        .bind_refs(&D1Type::Integer(limit))?
        .all()
        .await?;
    Ok(d1_results::<PendingAccountRow>(&result)?
        .into_iter()
        .map(PendingAccount::from)
        .collect())
}

/// Approve `account_id`; `false` when it was not pending, so two admins
/// reviewing at once cannot both act.
pub(crate) async fn approve_pending_account(db: &D1Database, account_id: &str) -> Result<bool> {
    let result = db
        .prepare("DELETE FROM pending_account_approvals WHERE account_id = ?1")
        .bind_refs(&D1Type::Text(account_id))?
        .run()
        .await?;
    Ok(result.meta()?.and_then(|meta| meta.changes).unwrap_or(0) > 0)
}

/// Delete a pending account and, through its foreign keys, its token,
/// password, keys and e-mail confirmation (Mastodon's reject frees the
/// username and e-mail). `false` when it was not pending.
pub(crate) async fn reject_pending_account(db: &D1Database, account_id: &str) -> Result<bool> {
    let result = db
        .prepare(
            "DELETE FROM accounts
             WHERE id = ?1
               AND EXISTS (
                   SELECT 1 FROM pending_account_approvals WHERE account_id = ?1
               )",
        )
        .bind_refs(&D1Type::Text(account_id))?
        .run()
        .await?;
    Ok(result.meta()?.and_then(|meta| meta.changes).unwrap_or(0) > 0)
}
