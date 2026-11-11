//! Reviewing sign-ups held for approval: Mastodon's
//! `POST /api/v1/admin/accounts/:id/approve` and `/reject`
//! (`Api::V1::Admin::AccountsController`), and the admin UI's queue.

use super::guard::{AdminAuthorization, authorize_admin_request};
use super::mastodon_admin::admin_record_not_found;
use crate::auth::{
    PendingAccount, approve_pending_account, find_account_by_id, find_pending_account,
    list_pending_accounts, reject_pending_account,
};
use crate::db_session::bind_request_d1;
use crate::responses::MastodonAccountResponse;
use crate::runtime_config::load_config;
use crate::time_html::timestamp_to_mastodon_iso8601;
use cfwdon_core::AppConfig;
use cfwdon_domain::{AccountApproval, ApprovalReview, LocalAccount, ReviewOutcome};
use worker::{Request, Response, Result, RouteContext};

const PENDING_QUEUE_LIMIT: u32 = 200;

/// Mastodon's `REST::Admin::AccountSerializer` for a local account. cfwdon
/// records no sign-in IPs, roles, or locale, and has no per-account
/// suspension, silence, or sensitive flag.
fn admin_account_document(
    config: &AppConfig,
    account: &LocalAccount,
    approved: bool,
    confirmed: bool,
    reason: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "id": account.id(),
        "username": account.username(),
        "domain": null,
        "created_at": timestamp_to_mastodon_iso8601(account.created_at()),
        "email": account.access_email(),
        "ip": null,
        "ips": [],
        "locale": null,
        "invite_request": reason,
        "role": null,
        "confirmed": confirmed,
        "approved": approved,
        "disabled": false,
        "silenced": false,
        "suspended": false,
        "sensitized": false,
        "account": MastodonAccountResponse::from_account(account, config),
    })
}

fn pending_account_document(config: &AppConfig, pending: &PendingAccount) -> serde_json::Value {
    admin_account_document(
        config,
        &pending.account,
        false,
        pending.confirmed,
        pending.reason.as_deref(),
    )
}

fn review_not_allowed() -> Result<Response> {
    Ok(
        Response::from_json(&serde_json::json!({ "error": "This action is not allowed" }))?
            .with_status(403),
    )
}

/// The sign-ups waiting for review, oldest first.
pub(crate) async fn admin_pending_accounts_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    if let AdminAuthorization::Denied(response) = authorize_admin_request(&req, &ctx).await? {
        return Ok(response);
    }
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    let documents = list_pending_accounts(&db, PENDING_QUEUE_LIMIT)
        .await?
        .iter()
        .map(|pending| pending_account_document(&config, pending))
        .collect::<Vec<_>>();
    Response::from_json(&documents)
}

pub(crate) async fn admin_account_approve_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    review_account_response(req, ctx, ApprovalReview::Approve).await
}

pub(crate) async fn admin_account_reject_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    review_account_response(req, ctx, ApprovalReview::Reject).await
}

async fn review_account_response(
    req: Request,
    ctx: RouteContext<()>,
    review: ApprovalReview,
) -> Result<Response> {
    if let AdminAuthorization::Denied(response) = authorize_admin_request(&req, &ctx).await? {
        return Ok(response);
    }
    let Some(account_id) = ctx.param("id").map(|id| id.trim().to_owned()) else {
        return admin_record_not_found();
    };
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    let Some(account) = find_account_by_id(&db, &account_id).await? else {
        return admin_record_not_found();
    };
    let pending = find_pending_account(&db, account.id()).await?;
    let approval = if pending.is_some() {
        AccountApproval::Pending
    } else {
        AccountApproval::Approved
    };
    // The conditional writes repeat the pending check, so a concurrent review
    // that got there first leaves this one refused.
    match approval.review(review) {
        None => review_not_allowed(),
        Some(ReviewOutcome::Approved) => {
            if !approve_pending_account(&db, account.id()).await? {
                return review_not_allowed();
            }
            let confirmed = pending.is_some_and(|pending| pending.confirmed);
            Response::from_json(&admin_account_document(
                &config, &account, true, confirmed, None,
            ))
        }
        Some(ReviewOutcome::Rejected) => {
            if !reject_pending_account(&db, account.id()).await? {
                return review_not_allowed();
            }
            Response::from_json(&serde_json::json!({}))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::admin_account_document;
    use cfwdon_core::AppConfig;
    use cfwdon_domain::{LocalAccount, LocalAccountRecord};

    #[test]
    fn admin_account_document_matches_mastodon_shape() {
        let config = AppConfig::new("example.com", "Example", "");
        let account =
            LocalAccount::from_record(LocalAccountRecord::test_fixture("acct-1", "alice"));
        let document = admin_account_document(&config, &account, false, true, Some("hi"));
        assert_eq!(document["id"], "acct-1");
        assert_eq!(document["username"], "alice");
        assert_eq!(document["domain"], serde_json::Value::Null);
        assert_eq!(document["email"], "alice@example.com");
        assert_eq!(document["approved"], false);
        assert_eq!(document["confirmed"], true);
        assert_eq!(document["invite_request"], "hi");
        assert_eq!(document["account"]["acct"], "alice");
    }
}
