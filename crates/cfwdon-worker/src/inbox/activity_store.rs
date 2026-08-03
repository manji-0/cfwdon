use super::RemoteActorProfile;
use worker::Result;
use worker::d1::D1Type;

pub(crate) const INBOX_IN_FLIGHT_STALE_MODIFIER: &str = "-15 minutes";

pub(crate) async fn begin_inbox_activity_processing(
    db: &crate::D1Database,
    actor_uri: &str,
    activity_id: &str,
    activity_type: &str,
) -> Result<bool> {
    let reclaim_bindings = [
        D1Type::Text(actor_uri),
        D1Type::Text(activity_id),
        D1Type::Text(INBOX_IN_FLIGHT_STALE_MODIFIER),
    ];
    db.prepare(
        "DELETE FROM inbox_activities
         WHERE actor_uri = ?1
           AND activity_id = ?2
           AND processed_at IS NULL
           AND created_at <= datetime(CURRENT_TIMESTAMP, ?3)",
    )
    .bind_refs(reclaim_bindings.iter())?
    .run()
    .await?;

    let bindings = [
        D1Type::Text(actor_uri),
        D1Type::Text(activity_id),
        D1Type::Text(activity_type),
    ];
    let row = db
        .prepare(
            "INSERT OR IGNORE INTO inbox_activities (
                actor_uri,
                activity_id,
                activity_type,
                created_at,
                processed_at
            ) VALUES (
                ?1,
                ?2,
                ?3,
                CURRENT_TIMESTAMP,
                NULL
            )
            RETURNING activity_id",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row.is_some())
}

pub(crate) async fn mark_inbox_activity_processed(
    db: &crate::D1Database,
    actor_uri: &str,
    activity_id: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(actor_uri), D1Type::Text(activity_id)];
    db.prepare(
        "UPDATE inbox_activities
         SET processed_at = CURRENT_TIMESTAMP
         WHERE actor_uri = ?1
           AND activity_id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InboxReclaimReport {
    pub marked_processed: u32,
    pub released: u32,
}

/// Reconcile inbox dedup rows left in-flight after worker timeouts.
pub(crate) async fn reclaim_stale_inbox_activities(
    db: &crate::D1Database,
    limit: u32,
) -> Result<InboxReclaimReport> {
    let limit = i32::try_from(limit).unwrap_or(100);
    let stale_modifier = INBOX_IN_FLIGHT_STALE_MODIFIER;

    let marked_bindings = [
        D1Type::Text(stale_modifier),
        D1Type::Text(stale_modifier),
        D1Type::Integer(limit),
    ];
    let marked = db
        .prepare(
            "UPDATE inbox_activities
             SET processed_at = CURRENT_TIMESTAMP
             WHERE processed_at IS NULL
               AND created_at <= datetime(CURRENT_TIMESTAMP, ?1)
               AND rowid IN (
                 SELECT ia.rowid
                 FROM inbox_activities ia
                 WHERE ia.processed_at IS NULL
                   AND ia.created_at <= datetime(CURRENT_TIMESTAMP, ?2)
                   AND (
                     (ia.activity_type = 'Create' AND EXISTS (
                       SELECT 1 FROM remote_statuses rs
                       WHERE rs.object_uri = REPLACE(ia.activity_id, '/activity', '')
                     ))
                     OR (ia.activity_type = 'Update' AND (
                       (ia.activity_id LIKE '%#updates/%' AND EXISTS (
                         SELECT 1 FROM remote_actors ra WHERE ra.actor_uri = ia.actor_uri
                       ))
                       OR EXISTS (
                         SELECT 1 FROM remote_statuses rs
                         WHERE rs.object_uri = REPLACE(ia.activity_id, '/activity', '')
                           OR rs.object_uri = ia.activity_id
                       )
                     ))
                     OR (ia.activity_type = 'Delete' AND NOT EXISTS (
                       SELECT 1 FROM remote_statuses rs
                       WHERE rs.object_uri = ia.activity_id
                          OR rs.url = ia.activity_id
                     ))
                   )
                 LIMIT ?3
               )",
        )
        .bind_refs(marked_bindings.iter())?
        .run()
        .await?;

    let released_bindings = [
        D1Type::Text(stale_modifier),
        D1Type::Text(stale_modifier),
        D1Type::Integer(limit),
    ];
    let released = db
        .prepare(
            "DELETE FROM inbox_activities
             WHERE processed_at IS NULL
               AND created_at <= datetime(CURRENT_TIMESTAMP, ?1)
               AND activity_type = 'Create'
               AND rowid IN (
                 SELECT ia.rowid
                 FROM inbox_activities ia
                 WHERE ia.processed_at IS NULL
                   AND ia.created_at <= datetime(CURRENT_TIMESTAMP, ?2)
                   AND ia.activity_type = 'Create'
                   AND NOT EXISTS (
                     SELECT 1 FROM remote_statuses rs
                     WHERE rs.object_uri = REPLACE(ia.activity_id, '/activity', '')
                   )
                 LIMIT ?3
               )",
        )
        .bind_refs(released_bindings.iter())?
        .run()
        .await?;

    Ok(InboxReclaimReport {
        marked_processed: marked.meta()?.and_then(|meta| meta.changes).unwrap_or(0) as u32,
        released: released.meta()?.and_then(|meta| meta.changes).unwrap_or(0) as u32,
    })
}

pub(crate) async fn release_inbox_activity_processing(
    db: &crate::D1Database,
    actor_uri: &str,
    activity_id: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(actor_uri), D1Type::Text(activity_id)];
    db.prepare(
        "DELETE FROM inbox_activities
         WHERE actor_uri = ?1
           AND activity_id = ?2
           AND processed_at IS NULL",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn upsert_follower_by_inbox(
    db: &crate::D1Database,
    account_id: &str,
    actor_uri: &str,
    inbox_uri: &str,
    shared_inbox_uri: Option<&str>,
    follow_activity_id: Option<&str>,
) -> Result<()> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(actor_uri),
        D1Type::Text(inbox_uri),
        match shared_inbox_uri {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        match follow_activity_id {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
    ];
    db.prepare(
        "INSERT INTO followers (
            id,
            account_id,
            actor_uri,
            inbox_uri,
            shared_inbox_uri,
            follow_activity_id,
            created_at,
            updated_at
        ) VALUES (
            lower(hex(randomblob(16))),
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(account_id, actor_uri) DO UPDATE SET
            inbox_uri = excluded.inbox_uri,
            shared_inbox_uri = excluded.shared_inbox_uri,
            follow_activity_id = COALESCE(excluded.follow_activity_id, followers.follow_activity_id),
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn upsert_follower(
    db: &crate::D1Database,
    account_id: &str,
    remote_actor: &RemoteActorProfile,
    follow_activity_id: Option<&str>,
) -> Result<()> {
    upsert_follower_by_inbox(
        db,
        account_id,
        &remote_actor.actor_uri,
        &remote_actor.inbox_uri,
        remote_actor.shared_inbox_uri.as_deref(),
        follow_activity_id,
    )
    .await
}

pub(crate) async fn find_follower_follow_activity_id(
    db: &crate::D1Database,
    account_id: &str,
    actor_uri: &str,
    canonical_actor_uri: &str,
) -> Result<Option<String>> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(actor_uri),
        D1Type::Text(canonical_actor_uri),
    ];
    let row = db
        .prepare(
            "SELECT follow_activity_id
             FROM followers
             WHERE account_id = ?1
               AND (actor_uri = ?2 OR actor_uri = ?3)
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row
        .as_ref()
        .and_then(|value| value.get("follow_activity_id"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned))
}

pub(crate) async fn delete_follower_by_actor(
    db: &crate::D1Database,
    account_id: &str,
    actor_uri: &str,
    canonical_actor_uri: &str,
) -> Result<()> {
    let lookup = [
        D1Type::Text(account_id),
        D1Type::Text(actor_uri),
        D1Type::Text(canonical_actor_uri),
    ];
    let inbox_row = db
        .prepare(
            "SELECT COALESCE(NULLIF(shared_inbox_uri, ''), inbox_uri) AS target_inbox
             FROM followers
             WHERE account_id = ?1
               AND (actor_uri = ?2 OR actor_uri = ?3)
             LIMIT 1",
        )
        .bind_refs(lookup.iter())?
        .first::<serde_json::Value>(None)
        .await?;

    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(actor_uri),
        D1Type::Text(canonical_actor_uri),
    ];
    db.prepare(
        "DELETE FROM followers
         WHERE account_id = ?1
           AND (actor_uri = ?2 OR actor_uri = ?3)",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    if let Some(target_inbox) = inbox_row
        .as_ref()
        .and_then(|value| value.get("target_inbox"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        crate::cancel_pending_outbox_deliveries_for_inbox(db, account_id, target_inbox).await?;
    }

    Ok(())
}
