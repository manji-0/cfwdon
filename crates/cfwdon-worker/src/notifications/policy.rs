use super::{
    Error, Request, Response, Result, RouteContext, resolve_authenticated_notification_context,
};
use serde::Deserialize;
use worker::d1::D1Type;

const DEFAULT_FOR_NOT_FOLLOWING: &str = "accept";
const DEFAULT_FOR_NOT_FOLLOWERS: &str = "accept";
const DEFAULT_FOR_NEW_ACCOUNTS: &str = "accept";
const DEFAULT_FOR_PRIVATE_MENTIONS: &str = "drop";
const DEFAULT_FOR_LIMITED_ACCOUNTS: &str = "filter";

#[derive(Debug, Deserialize)]
pub(crate) struct NotificationPolicyRow {
    pub(crate) for_not_following: String,
    pub(crate) for_not_followers: String,
    pub(crate) for_new_accounts: String,
    pub(crate) for_private_mentions: String,
    pub(crate) for_limited_accounts: String,
}

#[derive(Debug, Default, Deserialize)]
struct UpdateNotificationPolicyRequest {
    for_not_following: Option<String>,
    for_not_followers: Option<String>,
    for_new_accounts: Option<String>,
    for_private_mentions: Option<String>,
    for_limited_accounts: Option<String>,
}

fn normalize_policy_value(value: &str, field: &str) -> std::result::Result<String, Error> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "accept" | "filter" | "drop" => Ok(normalized),
        _ => Err(Error::RustError(format!(
            "{field} must be one of: accept, filter, drop"
        ))),
    }
}

fn build_notification_policy_document(row: &NotificationPolicyRow) -> serde_json::Value {
    serde_json::json!({
        "for_not_following": row.for_not_following,
        "for_not_followers": row.for_not_followers,
        "for_new_accounts": row.for_new_accounts,
        "for_private_mentions": row.for_private_mentions,
        "for_limited_accounts": row.for_limited_accounts,
        "summary": {
            "pending_requests_count": 0,
            "pending_notifications_count": 0,
        }
    })
}

pub(crate) async fn load_notification_policy_row(
    db: &worker::D1Database,
    account_id: &str,
) -> Result<NotificationPolicyRow> {
    let account_id = D1Type::Text(account_id);
    let row = db
        .prepare(
            "SELECT for_not_following, for_not_followers, for_new_accounts,
                    for_private_mentions, for_limited_accounts
             FROM notification_policies
             WHERE account_id = ?1
             LIMIT 1",
        )
        .bind_refs(&account_id)?
        .first::<NotificationPolicyRow>(None)
        .await?;

    Ok(row.unwrap_or(NotificationPolicyRow {
        for_not_following: DEFAULT_FOR_NOT_FOLLOWING.to_owned(),
        for_not_followers: DEFAULT_FOR_NOT_FOLLOWERS.to_owned(),
        for_new_accounts: DEFAULT_FOR_NEW_ACCOUNTS.to_owned(),
        for_private_mentions: DEFAULT_FOR_PRIVATE_MENTIONS.to_owned(),
        for_limited_accounts: DEFAULT_FOR_LIMITED_ACCOUNTS.to_owned(),
    }))
}

async fn parse_update_notification_policy_request(
    req: &mut Request,
) -> std::result::Result<UpdateNotificationPolicyRequest, Error> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| Error::RustError(format!("failed to read Content-Type header: {error}")))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    if content_type.contains("application/json") {
        return req
            .json::<UpdateNotificationPolicyRequest>()
            .await
            .map_err(|error| {
                Error::RustError(format!("invalid JSON notification policy payload: {error}"))
            });
    }

    let body = req.text().await.map_err(|error| {
        Error::RustError(format!("invalid notification policy payload: {error}"))
    })?;
    let mut parsed = UpdateNotificationPolicyRequest::default();
    for (key, value) in url::form_urlencoded::parse(body.as_bytes()) {
        let value = Some(value.into_owned());
        match key.as_ref() {
            "for_not_following" => parsed.for_not_following = value,
            "for_not_followers" => parsed.for_not_followers = value,
            "for_new_accounts" => parsed.for_new_accounts = value,
            "for_private_mentions" => parsed.for_private_mentions = value,
            "for_limited_accounts" => parsed.for_limited_accounts = value,
            _ => {}
        }
    }
    Ok(parsed)
}

pub(crate) async fn notifications_policy_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let Some(auth) = resolve_authenticated_notification_context(&req, &ctx).await? else {
        return Response::error("Auth0 authentication required", 401);
    };

    let row = load_notification_policy_row(&auth.db, auth.viewer.id()).await?;
    Response::from_json(&build_notification_policy_document(&row))
}

pub(crate) async fn update_notifications_policy_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let Some(auth) = resolve_authenticated_notification_context(req, &ctx).await? else {
        return Response::error("Auth0 authentication required", 401);
    };

    let update = parse_update_notification_policy_request(req).await?;
    let current = load_notification_policy_row(&auth.db, auth.viewer.id()).await?;
    let next = NotificationPolicyRow {
        for_not_following: match update.for_not_following.as_deref() {
            Some(value) => normalize_policy_value(value, "for_not_following")?,
            None => current.for_not_following,
        },
        for_not_followers: match update.for_not_followers.as_deref() {
            Some(value) => normalize_policy_value(value, "for_not_followers")?,
            None => current.for_not_followers,
        },
        for_new_accounts: match update.for_new_accounts.as_deref() {
            Some(value) => normalize_policy_value(value, "for_new_accounts")?,
            None => current.for_new_accounts,
        },
        for_private_mentions: match update.for_private_mentions.as_deref() {
            Some(value) => normalize_policy_value(value, "for_private_mentions")?,
            None => current.for_private_mentions,
        },
        for_limited_accounts: match update.for_limited_accounts.as_deref() {
            Some(value) => normalize_policy_value(value, "for_limited_accounts")?,
            None => current.for_limited_accounts,
        },
    };

    let bindings = [
        D1Type::Text(auth.viewer.id()),
        D1Type::Text(next.for_not_following.as_str()),
        D1Type::Text(next.for_not_followers.as_str()),
        D1Type::Text(next.for_new_accounts.as_str()),
        D1Type::Text(next.for_private_mentions.as_str()),
        D1Type::Text(next.for_limited_accounts.as_str()),
    ];
    auth.db
        .prepare(
            "INSERT INTO notification_policies (
            account_id,
            for_not_following,
            for_not_followers,
            for_new_accounts,
            for_private_mentions,
            for_limited_accounts,
            updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, CURRENT_TIMESTAMP
        )
        ON CONFLICT(account_id) DO UPDATE SET
            for_not_following = excluded.for_not_following,
            for_not_followers = excluded.for_not_followers,
            for_new_accounts = excluded.for_new_accounts,
            for_private_mentions = excluded.for_private_mentions,
            for_limited_accounts = excluded.for_limited_accounts,
            updated_at = CURRENT_TIMESTAMP",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;

    Response::from_json(&build_notification_policy_document(&next))
}
