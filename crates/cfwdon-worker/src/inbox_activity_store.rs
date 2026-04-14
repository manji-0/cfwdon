use super::RemoteActorProfile;
use worker::Result;
use worker::d1::D1Type;

pub(crate) async fn begin_inbox_activity_processing(
    db: &worker::D1Database,
    actor_uri: &str,
    activity_id: &str,
    activity_type: &str,
) -> Result<bool> {
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
    db: &worker::D1Database,
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

pub(crate) async fn release_inbox_activity_processing(
    db: &worker::D1Database,
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

pub(crate) async fn upsert_follower(
    db: &worker::D1Database,
    account_id: &str,
    remote_actor: &RemoteActorProfile,
) -> Result<()> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(remote_actor.actor_uri.as_str()),
        D1Type::Text(remote_actor.inbox_uri.as_str()),
        match remote_actor.shared_inbox_uri.as_deref() {
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
            created_at,
            updated_at
        ) VALUES (
            lower(hex(randomblob(16))),
            ?1,
            ?2,
            ?3,
            ?4,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(account_id, actor_uri) DO UPDATE SET
            inbox_uri = excluded.inbox_uri,
            shared_inbox_uri = excluded.shared_inbox_uri,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn delete_follower_by_actor(
    db: &worker::D1Database,
    account_id: &str,
    actor_uri: &str,
    canonical_actor_uri: &str,
) -> Result<()> {
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

    Ok(())
}
