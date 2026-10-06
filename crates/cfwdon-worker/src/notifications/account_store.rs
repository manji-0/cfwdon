use super::{NotificationTimeWindow, StoredTimestampFormat};
use crate::db_utils::d1_results;
use crate::tracked_d1::D1Database;
use cfwdon_domain::{LocalAccount, LocalAccountRecord};
use serde::Deserialize;
use worker::Result;
use worker::d1::D1Type;
#[derive(Debug, Deserialize)]
pub(crate) struct LocalFollowNotificationRow {
    pub(crate) follower_account_id: String,
    pub(crate) created_at: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RemoteFollowNotificationRow {
    pub(crate) actor_uri: String,
    pub(crate) created_at: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LocalFollowRequestNotificationRow {
    pub(crate) follower_account_id: String,
    pub(crate) created_at: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RemoteFollowRequestNotificationRow {
    pub(crate) actor_uri: String,
    pub(crate) created_at: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FavouriteNotificationRow {
    pub(crate) account_id: String,
    pub(crate) status_id: String,
    pub(crate) created_at: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RemoteStatusInteractionRow {
    pub(crate) remote_actor_uri: String,
    pub(crate) status_id: String,
    pub(crate) created_at: String,
}

pub(crate) async fn list_local_follow_request_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
    window: &NotificationTimeWindow,
) -> Result<Vec<LocalFollowRequestNotificationRow>> {
    let bounds = window.sql_bounds(StoredTimestampFormat::Sqlite);
    let mut bindings = vec![D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    bindings.extend(bounds.bindings());
    let result = db
        .prepare(format!(
            "SELECT follower_account_id, created_at
             FROM follows
             WHERE target_account_id = ?1
               AND state = 'pending'{}
             ORDER BY created_at DESC
             LIMIT ?2",
            bounds.clause("created_at", 3)
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    d1_results::<LocalFollowRequestNotificationRow>(&result)
}

pub(crate) async fn list_remote_follow_request_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
    window: &NotificationTimeWindow,
) -> Result<Vec<RemoteFollowRequestNotificationRow>> {
    let bounds = window.sql_bounds(StoredTimestampFormat::Sqlite);
    let mut bindings = vec![D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    bindings.extend(bounds.bindings());
    let result = db
        .prepare(format!(
            "SELECT requester_actor_uri AS actor_uri, created_at
             FROM follow_requests
             WHERE account_id = ?1{}
             ORDER BY created_at DESC
             LIMIT ?2",
            bounds.clause("created_at", 3)
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    d1_results::<RemoteFollowRequestNotificationRow>(&result)
}

pub(crate) async fn list_local_follow_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
    window: &NotificationTimeWindow,
) -> Result<Vec<LocalFollowNotificationRow>> {
    let bounds = window.sql_bounds(StoredTimestampFormat::Sqlite);
    let mut bindings = vec![D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    bindings.extend(bounds.bindings());
    let result = db
        .prepare(format!(
            "SELECT follower_account_id, created_at
             FROM follows
             WHERE target_account_id = ?1
               AND state = 'accepted'{}
             ORDER BY created_at DESC
             LIMIT ?2",
            bounds.clause("created_at", 3)
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    d1_results::<LocalFollowNotificationRow>(&result)
}

pub(crate) async fn list_admin_sign_up_notifications(
    db: &D1Database,
    admin_account_id: &str,
    limit: u32,
) -> Result<Vec<LocalAccount>> {
    let bindings = [
        D1Type::Text(admin_account_id),
        D1Type::Integer(limit as i32),
    ];
    let result = db
        .prepare(
            "SELECT id, username, access_email, display_name, bio_html, bio_text, fields_json, locked, bot, discoverable, default_post_visibility, default_quote_policy, default_sensitive, default_language, avatar_object_key, avatar_content_type, header_object_key, header_content_type, '' AS private_key_jwk, public_key_pem, created_at
             FROM accounts
             WHERE id != ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    Ok(d1_results::<LocalAccountRecord>(&result)?
        .into_iter()
        .map(LocalAccount::from_record)
        .collect())
}

pub(crate) async fn list_remote_follow_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
    window: &NotificationTimeWindow,
) -> Result<Vec<RemoteFollowNotificationRow>> {
    let bounds = window.sql_bounds(StoredTimestampFormat::Sqlite);
    let mut bindings = vec![D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    bindings.extend(bounds.bindings());
    let result = db
        .prepare(format!(
            "SELECT actor_uri, created_at
             FROM followers
             WHERE account_id = ?1{}
             ORDER BY created_at DESC
             LIMIT ?2",
            bounds.clause("created_at", 3)
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    d1_results::<RemoteFollowNotificationRow>(&result)
}

pub(crate) async fn list_favourite_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
    window: &NotificationTimeWindow,
) -> Result<Vec<FavouriteNotificationRow>> {
    let bounds = window.sql_bounds(StoredTimestampFormat::Sqlite);
    let mut bindings = vec![D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    bindings.extend(bounds.bindings());
    let result = db
        .prepare(format!(
            "SELECT f.account_id, f.status_id, f.created_at
             FROM favourites f
             JOIN statuses s
               ON s.id = f.status_id
             WHERE s.account_id = ?1
               AND f.account_id != ?1
               AND f.status_id IS NOT NULL{}
             ORDER BY f.created_at DESC
             LIMIT ?2",
            bounds.clause("f.created_at", 3)
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    d1_results::<FavouriteNotificationRow>(&result)
}

pub(crate) async fn list_remote_favourite_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
    window: &NotificationTimeWindow,
) -> Result<Vec<RemoteStatusInteractionRow>> {
    let bounds = window.sql_bounds(StoredTimestampFormat::Sqlite);
    let mut bindings = vec![D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    bindings.extend(bounds.bindings());
    let result = db
        .prepare(format!(
            "SELECT rf.remote_actor_uri, rf.status_id, rf.created_at
             FROM remote_favourites rf
             JOIN statuses s
               ON s.id = rf.status_id
             WHERE s.account_id = ?1{}
             ORDER BY rf.created_at DESC
             LIMIT ?2",
            bounds.clause("rf.created_at", 3)
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    d1_results::<RemoteStatusInteractionRow>(&result)
}

pub(crate) async fn list_remote_reblog_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
    window: &NotificationTimeWindow,
) -> Result<Vec<RemoteStatusInteractionRow>> {
    let bounds = window.sql_bounds(StoredTimestampFormat::Sqlite);
    let mut bindings = vec![D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    bindings.extend(bounds.bindings());
    let result = db
        .prepare(format!(
            "SELECT rr.remote_actor_uri, rr.status_id, rr.created_at
             FROM remote_reblogs rr
             JOIN statuses s
               ON s.id = rr.status_id
             WHERE s.account_id = ?1{}
             ORDER BY rr.created_at DESC
             LIMIT ?2",
            bounds.clause("rr.created_at", 3)
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    d1_results::<RemoteStatusInteractionRow>(&result)
}
