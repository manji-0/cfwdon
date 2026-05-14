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

pub(crate) async fn upsert_follower_by_inbox(
    db: &worker::D1Database,
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
    db: &worker::D1Database,
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
    db: &worker::D1Database,
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
