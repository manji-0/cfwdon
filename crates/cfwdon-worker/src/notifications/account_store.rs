use super::AccountRow;
use cfwdon_domain::LocalAccount;
use serde::Deserialize;
use worker::Result;
use worker::d1::D1Type;

use crate::D1Database;
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
) -> Result<Vec<LocalFollowRequestNotificationRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT follower_account_id, created_at
             FROM follows
             WHERE target_account_id = ?1
               AND state = 'pending'
             ORDER BY created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<LocalFollowRequestNotificationRow>()
}

pub(crate) async fn list_remote_follow_request_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<RemoteFollowRequestNotificationRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT requester_actor_uri AS actor_uri, created_at
             FROM follow_requests
             WHERE account_id = ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<RemoteFollowRequestNotificationRow>()
}

pub(crate) async fn list_local_follow_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<LocalFollowNotificationRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT follower_account_id, created_at
             FROM follows
             WHERE target_account_id = ?1
               AND state = 'accepted'
             ORDER BY created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<LocalFollowNotificationRow>()
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

    Ok(result
        .results::<AccountRow>()?
        .into_iter()
        .map(LocalAccount::from_record)
        .collect())
}

pub(crate) async fn list_remote_follow_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<RemoteFollowNotificationRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT actor_uri, created_at
             FROM followers
             WHERE account_id = ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<RemoteFollowNotificationRow>()
}

pub(crate) async fn list_favourite_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
    min_created_at: Option<&str>,
) -> Result<Vec<FavouriteNotificationRow>> {
    let result = if let Some(min_created_at) = min_created_at {
        let bindings = [
            D1Type::Text(account_id),
            D1Type::Text(min_created_at),
            D1Type::Integer(limit as i32),
        ];
        db.prepare(
            "SELECT f.account_id, f.status_id, f.created_at
             FROM favourites f
             JOIN statuses s
               ON s.id = f.status_id
             WHERE s.account_id = ?1
               AND f.account_id != ?1
               AND f.status_id IS NOT NULL
               AND f.created_at >= ?2
             ORDER BY f.created_at DESC
             LIMIT ?3",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?
    } else {
        let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
        db.prepare(
            "SELECT f.account_id, f.status_id, f.created_at
             FROM favourites f
             JOIN statuses s
               ON s.id = f.status_id
             WHERE s.account_id = ?1
               AND f.account_id != ?1
               AND f.status_id IS NOT NULL
             ORDER BY f.created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?
    };

    result.results::<FavouriteNotificationRow>()
}

pub(crate) async fn list_remote_favourite_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
    min_created_at: Option<&str>,
) -> Result<Vec<RemoteStatusInteractionRow>> {
    let result = if let Some(min_created_at) = min_created_at {
        let bindings = [
            D1Type::Text(account_id),
            D1Type::Text(min_created_at),
            D1Type::Integer(limit as i32),
        ];
        db.prepare(
            "SELECT rf.remote_actor_uri, rf.status_id, rf.created_at
             FROM remote_favourites rf
             JOIN statuses s
               ON s.id = rf.status_id
             WHERE s.account_id = ?1
               AND rf.created_at >= ?2
             ORDER BY rf.created_at DESC
             LIMIT ?3",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?
    } else {
        let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
        db.prepare(
            "SELECT rf.remote_actor_uri, rf.status_id, rf.created_at
             FROM remote_favourites rf
             JOIN statuses s
               ON s.id = rf.status_id
             WHERE s.account_id = ?1
             ORDER BY rf.created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?
    };

    result.results::<RemoteStatusInteractionRow>()
}

pub(crate) async fn list_remote_reblog_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<RemoteStatusInteractionRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT rr.remote_actor_uri, rr.status_id, rr.created_at
             FROM remote_reblogs rr
             JOIN statuses s
               ON s.id = rr.status_id
             WHERE s.account_id = ?1
             ORDER BY rr.created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<RemoteStatusInteractionRow>()
}
