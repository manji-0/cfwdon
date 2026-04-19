use super::{AccountRow, FollowerTargetRow, LocalAccount, RemoteActorRow, UsernameRow, count_rows};
use worker::d1::D1Type;
use worker::{D1Database, Result};

#[derive(Debug, serde::Deserialize)]
pub(crate) struct LocalFollowAccountEntryRow {
    pub(crate) cursor_id: i64,
    pub(crate) account_id: String,
    pub(crate) created_at: String,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct RemoteFollowAccountEntryRow {
    pub(crate) cursor_id: i64,
    pub(crate) actor_uri: String,
    pub(crate) created_at: String,
}

pub(crate) async fn count_followers_by_actor(
    db: &D1Database,
    account_id: &str,
    actor_uri: &str,
) -> Result<u64> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(actor_uri)];
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM followers
             WHERE account_id = ?1
               AND actor_uri = ?2",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row
        .as_ref()
        .and_then(|value| value.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0))
}

pub(crate) async fn list_local_follower_usernames(
    db: &D1Database,
    account_id: &str,
) -> Result<Vec<String>> {
    let account_id = D1Type::Text(account_id);
    let result = db
        .prepare(
            "SELECT a.username
             FROM follows f
             JOIN accounts a ON a.id = f.follower_account_id
             WHERE f.target_account_id = ?1
               AND f.state = 'accepted'
             ORDER BY f.created_at ASC",
        )
        .bind_refs(&account_id)?
        .all()
        .await?;

    Ok(result
        .results::<UsernameRow>()?
        .into_iter()
        .map(|row| row.username)
        .collect())
}

pub(crate) async fn list_following_actor_uris(
    db: &D1Database,
    account_id: &str,
) -> Result<Vec<String>> {
    let account_id = D1Type::Text(account_id);
    let result = db
        .prepare(
            "SELECT target_actor_uri AS target_inbox
             FROM follows
             WHERE follower_account_id = ?1
               AND state = 'accepted'
             ORDER BY created_at ASC",
        )
        .bind_refs(&account_id)?
        .all()
        .await?;

    Ok(result
        .results::<FollowerTargetRow>()?
        .into_iter()
        .map(|row| row.target_inbox)
        .filter(|value| !value.trim().is_empty())
        .collect())
}

pub(crate) async fn count_accepted_following(db: &D1Database, account_id: &str) -> Result<u64> {
    count_rows(
        db,
        "SELECT COUNT(*) AS count
         FROM follows
         WHERE follower_account_id = ?1
           AND state = 'accepted'",
        account_id,
    )
    .await
}

pub(crate) async fn count_local_followers(db: &D1Database, account_id: &str) -> Result<u64> {
    count_rows(
        db,
        "SELECT COUNT(*) AS count
         FROM follows
         WHERE target_account_id = ?1
           AND state = 'accepted'",
        account_id,
    )
    .await
}

pub(crate) async fn count_remote_followers(db: &D1Database, account_id: &str) -> Result<u64> {
    count_rows(
        db,
        "SELECT COUNT(*) AS count
         FROM followers
         WHERE account_id = ?1",
        account_id,
    )
    .await
}

pub(crate) async fn has_any_local_followers_for_remote_actor(
    db: &D1Database,
    actor_uri: &str,
) -> Result<bool> {
    Ok(count_rows(
        db,
        "SELECT COUNT(*) AS count
         FROM follows
         WHERE target_actor_uri = ?1
           AND state = 'accepted'",
        actor_uri,
    )
    .await?
        > 0)
}

pub(crate) async fn is_local_account_following_remote_actor(
    db: &D1Database,
    account_id: &str,
    actor_uri: &str,
) -> Result<bool> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(actor_uri)];
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM follows
             WHERE follower_account_id = ?1
               AND target_actor_uri = ?2
               AND state = 'accepted'",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row
        .as_ref()
        .and_then(|value| value.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0)
}

pub(crate) async fn first_local_follower_for_remote_actor(
    db: &D1Database,
    actor_uri: &str,
) -> Result<Option<LocalAccount>> {
    let bindings = [D1Type::Text(actor_uri)];
    let row = db
        .prepare(
            "SELECT a.id, a.username, a.access_email, a.display_name, a.bio_html, a.bio_text, a.fields_json, a.discoverable, a.default_post_visibility, a.default_sensitive, a.default_language, a.avatar_object_key, a.avatar_content_type, a.header_object_key, a.header_content_type, a.private_key_jwk, a.public_key_pem, a.created_at
             FROM follows f
             JOIN accounts a
               ON a.id = f.follower_account_id
             WHERE f.target_actor_uri = ?1
               AND f.state = 'accepted'
             ORDER BY f.created_at ASC
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<AccountRow>(None)
        .await?;

    Ok(row.map(LocalAccount::from))
}

pub(crate) async fn is_local_follower_authorized(
    db: &D1Database,
    viewer_account_id: &str,
    owner_account_id: &str,
) -> Result<bool> {
    let owner = D1Type::Text(owner_account_id);
    let viewer = D1Type::Text(viewer_account_id);
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM follows
             WHERE follower_account_id = ?2
               AND target_account_id = ?1
               AND state = 'accepted'",
        )
        .bind_refs(&[owner, viewer])?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row
        .as_ref()
        .and_then(|value| value.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0)
}

pub(crate) async fn list_familiar_local_accounts_for_local_target(
    db: &D1Database,
    viewer_account_id: &str,
    target_account_id: &str,
    limit: u32,
) -> Result<Vec<LocalAccount>> {
    let bindings = [
        D1Type::Text(viewer_account_id),
        D1Type::Text(target_account_id),
        D1Type::Integer(limit as i32),
    ];
    let result = db
        .prepare(
            "SELECT DISTINCT a.id, a.username, a.access_email, a.display_name, a.bio_html, a.bio_text, a.fields_json, a.discoverable, a.default_post_visibility, a.default_sensitive, a.default_language, a.avatar_object_key, a.avatar_content_type, a.header_object_key, a.header_content_type, a.private_key_jwk, a.public_key_pem, a.created_at
             FROM follows viewer_follows
             JOIN follows familiar_follows
               ON familiar_follows.follower_account_id = viewer_follows.target_account_id
             JOIN accounts a
               ON a.id = familiar_follows.follower_account_id
             WHERE viewer_follows.follower_account_id = ?1
               AND viewer_follows.state = 'accepted'
               AND familiar_follows.target_account_id = ?2
               AND familiar_follows.state = 'accepted'
             ORDER BY a.username ASC
             LIMIT ?3",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    Ok(result
        .results::<AccountRow>()?
        .into_iter()
        .map(LocalAccount::from)
        .collect())
}

pub(crate) async fn list_familiar_remote_actors_for_local_target(
    db: &D1Database,
    viewer_account_id: &str,
    target_account_id: &str,
    limit: u32,
) -> Result<Vec<RemoteActorRow>> {
    let bindings = [
        D1Type::Text(viewer_account_id),
        D1Type::Text(target_account_id),
        D1Type::Integer(limit as i32),
    ];
    let result = db
        .prepare(
            "SELECT DISTINCT ra.actor_uri, ra.username, ra.domain, ra.locked, ra.bot, ra.display_name, ra.summary_html, ra.profile_url, ra.avatar_url, ra.header_url
             FROM follows viewer_follows
             JOIN followers remote_followers
               ON remote_followers.actor_uri = viewer_follows.target_actor_uri
             JOIN remote_actors ra
               ON ra.actor_uri = remote_followers.actor_uri
             WHERE viewer_follows.follower_account_id = ?1
               AND viewer_follows.state = 'accepted'
               AND remote_followers.account_id = ?2
             ORDER BY ra.username ASC, ra.domain ASC
             LIMIT ?3",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    Ok(result.results::<RemoteActorRow>()?)
}

pub(crate) async fn list_familiar_local_accounts_for_remote_target(
    db: &D1Database,
    viewer_account_id: &str,
    target_actor_uri: &str,
    limit: u32,
) -> Result<Vec<LocalAccount>> {
    let bindings = [
        D1Type::Text(viewer_account_id),
        D1Type::Text(target_actor_uri),
        D1Type::Integer(limit as i32),
    ];
    let result = db
        .prepare(
            "SELECT DISTINCT a.id, a.username, a.access_email, a.display_name, a.bio_html, a.bio_text, a.fields_json, a.discoverable, a.default_post_visibility, a.default_sensitive, a.default_language, a.avatar_object_key, a.avatar_content_type, a.header_object_key, a.header_content_type, a.private_key_jwk, a.public_key_pem, a.created_at
             FROM follows viewer_follows
             JOIN follows familiar_follows
               ON familiar_follows.follower_account_id = viewer_follows.target_account_id
             JOIN accounts a
               ON a.id = familiar_follows.follower_account_id
             WHERE viewer_follows.follower_account_id = ?1
               AND viewer_follows.state = 'accepted'
               AND familiar_follows.target_actor_uri = ?2
               AND familiar_follows.state = 'accepted'
             ORDER BY a.username ASC
             LIMIT ?3",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    Ok(result
        .results::<AccountRow>()?
        .into_iter()
        .map(LocalAccount::from)
        .collect())
}

pub(crate) async fn list_local_followers_for_account(
    db: &D1Database,
    account_id: &str,
) -> Result<Vec<LocalFollowAccountEntryRow>> {
    let account_id = D1Type::Text(account_id);
    let result = db
        .prepare(
            "SELECT rowid AS cursor_id, follower_account_id AS account_id, created_at
             FROM follows
             WHERE target_account_id = ?1
               AND state = 'accepted'
             ORDER BY created_at DESC, rowid DESC",
        )
        .bind_refs(&account_id)?
        .all()
        .await?;
    result.results::<LocalFollowAccountEntryRow>()
}

pub(crate) async fn list_remote_followers_for_account(
    db: &D1Database,
    account_id: &str,
) -> Result<Vec<RemoteFollowAccountEntryRow>> {
    let account_id = D1Type::Text(account_id);
    let result = db
        .prepare(
            "SELECT rowid AS cursor_id, actor_uri, created_at
             FROM followers
             WHERE account_id = ?1
             ORDER BY created_at DESC, rowid DESC",
        )
        .bind_refs(&account_id)?
        .all()
        .await?;
    result.results::<RemoteFollowAccountEntryRow>()
}

pub(crate) async fn list_local_following_for_account(
    db: &D1Database,
    account_id: &str,
) -> Result<Vec<LocalFollowAccountEntryRow>> {
    let account_id = D1Type::Text(account_id);
    let result = db
        .prepare(
            "SELECT rowid AS cursor_id, target_account_id AS account_id, created_at
             FROM follows
             WHERE follower_account_id = ?1
               AND target_account_id IS NOT NULL
               AND state = 'accepted'
             ORDER BY created_at DESC, rowid DESC",
        )
        .bind_refs(&account_id)?
        .all()
        .await?;
    result.results::<LocalFollowAccountEntryRow>()
}

pub(crate) async fn list_remote_following_for_account(
    db: &D1Database,
    account_id: &str,
) -> Result<Vec<RemoteFollowAccountEntryRow>> {
    let account_id = D1Type::Text(account_id);
    let result = db
        .prepare(
            "SELECT rowid AS cursor_id, target_actor_uri AS actor_uri, created_at
             FROM follows
             WHERE follower_account_id = ?1
               AND target_account_id IS NULL
               AND state = 'accepted'
             ORDER BY created_at DESC, rowid DESC",
        )
        .bind_refs(&account_id)?
        .all()
        .await?;
    result.results::<RemoteFollowAccountEntryRow>()
}

pub(crate) async fn list_local_followers_for_remote_actor(
    db: &D1Database,
    actor_uri: &str,
) -> Result<Vec<LocalFollowAccountEntryRow>> {
    let actor_uri = D1Type::Text(actor_uri);
    let result = db
        .prepare(
            "SELECT rowid AS cursor_id, follower_account_id AS account_id, created_at
             FROM follows
             WHERE target_actor_uri = ?1
               AND state = 'accepted'
             ORDER BY created_at DESC, rowid DESC",
        )
        .bind_refs(&actor_uri)?
        .all()
        .await?;
    result.results::<LocalFollowAccountEntryRow>()
}

pub(crate) async fn list_local_following_for_remote_actor(
    db: &D1Database,
    actor_uri: &str,
) -> Result<Vec<LocalFollowAccountEntryRow>> {
    let actor_uri = D1Type::Text(actor_uri);
    let result = db
        .prepare(
            "SELECT rowid AS cursor_id, account_id, created_at
             FROM followers
             WHERE actor_uri = ?1
             ORDER BY created_at DESC, rowid DESC",
        )
        .bind_refs(&actor_uri)?
        .all()
        .await?;
    result.results::<LocalFollowAccountEntryRow>()
}
