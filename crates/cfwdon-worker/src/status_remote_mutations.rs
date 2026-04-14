use worker::d1::D1Type;
use worker::{D1Database, Result};

pub(crate) async fn delete_remote_status_by_id(db: &D1Database, status_id: &str) -> Result<()> {
    let bindings = [D1Type::Text(status_id)];
    db.prepare(
        "DELETE FROM remote_status_poll_options
         WHERE poll_id IN (
            SELECT id
            FROM remote_status_polls
            WHERE status_id = ?1
         )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    db.prepare(
        "DELETE FROM remote_status_polls
         WHERE status_id = ?1",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    let status_id = D1Type::Text(status_id);
    db.prepare(
        "DELETE FROM remote_statuses
         WHERE id = ?1",
    )
    .bind_refs(&status_id)?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn upsert_remote_favourite(
    db: &D1Database,
    remote_actor_uri: &str,
    status_id: &str,
    target_uri: &str,
    activity_uri: Option<&str>,
) -> Result<()> {
    let bindings = [
        D1Type::Text(remote_actor_uri),
        D1Type::Text(status_id),
        D1Type::Text(target_uri),
        match activity_uri {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
    ];
    db.prepare(
        "INSERT INTO remote_favourites (
            remote_actor_uri,
            status_id,
            target_uri,
            activity_uri,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(remote_actor_uri, target_uri) DO UPDATE SET
            status_id = excluded.status_id,
            activity_uri = excluded.activity_uri,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn delete_remote_favourite(
    db: &D1Database,
    remote_actor_uri: &str,
    target_uri: &str,
    activity_uri: Option<&str>,
) -> Result<()> {
    match activity_uri {
        Some(activity_uri) => {
            let bindings = [D1Type::Text(remote_actor_uri), D1Type::Text(activity_uri)];
            db.prepare(
                "DELETE FROM remote_favourites
                 WHERE remote_actor_uri = ?1
                   AND activity_uri = ?2",
            )
            .bind_refs(bindings.iter())?
            .run()
            .await?;
        }
        None => {
            let bindings = [D1Type::Text(remote_actor_uri), D1Type::Text(target_uri)];
            db.prepare(
                "DELETE FROM remote_favourites
                 WHERE remote_actor_uri = ?1
                   AND target_uri = ?2",
            )
            .bind_refs(bindings.iter())?
            .run()
            .await?;
        }
    }

    Ok(())
}

pub(crate) async fn upsert_remote_reblog(
    db: &D1Database,
    remote_actor_uri: &str,
    status_id: &str,
    target_uri: &str,
    activity_uri: Option<&str>,
) -> Result<()> {
    let bindings = [
        D1Type::Text(remote_actor_uri),
        D1Type::Text(status_id),
        D1Type::Text(target_uri),
        match activity_uri {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
    ];
    db.prepare(
        "INSERT INTO remote_reblogs (
            remote_actor_uri,
            status_id,
            target_uri,
            activity_uri,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(remote_actor_uri, target_uri) DO UPDATE SET
            status_id = excluded.status_id,
            activity_uri = excluded.activity_uri,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn delete_remote_reblog(
    db: &D1Database,
    remote_actor_uri: &str,
    target_uri: &str,
    activity_uri: Option<&str>,
) -> Result<()> {
    match activity_uri {
        Some(activity_uri) => {
            let bindings = [D1Type::Text(remote_actor_uri), D1Type::Text(activity_uri)];
            db.prepare(
                "DELETE FROM remote_reblogs
                 WHERE remote_actor_uri = ?1
                   AND activity_uri = ?2",
            )
            .bind_refs(bindings.iter())?
            .run()
            .await?;
        }
        None => {
            let bindings = [D1Type::Text(remote_actor_uri), D1Type::Text(target_uri)];
            db.prepare(
                "DELETE FROM remote_reblogs
                 WHERE remote_actor_uri = ?1
                   AND target_uri = ?2",
            )
            .bind_refs(bindings.iter())?
            .run()
            .await?;
        }
    }

    Ok(())
}
