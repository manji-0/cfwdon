use crate::{FollowAccountRequest, FollowRow, LocalAccount, RemoteActorProfile, RemoteActorRow};
use worker::d1::D1Type;
use worker::{D1Database, Error, Result};

pub(crate) async fn find_follow_by_activity_id(
    db: &D1Database,
    follow_activity_id: &str,
) -> Result<Option<FollowRow>> {
    let follow_activity_id = D1Type::Text(follow_activity_id);
    db.prepare(
        "SELECT follower_account_id, target_account_id, target_actor_uri, follow_activity_id, state, show_reblogs, notify, languages_json
         FROM follows
         WHERE follow_activity_id = ?1
         LIMIT 1",
    )
    .bind_refs(&follow_activity_id)?
    .first::<FollowRow>(None)
    .await
}

pub(crate) async fn update_follow_state_from_response(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    state: &str,
) -> Result<()> {
    let Some(follow_activity_id) = activity
        .get("object")
        .and_then(|object| object.get("id"))
        .and_then(serde_json::Value::as_str)
    else {
        return Ok(());
    };
    let Some(follow) = find_follow_by_activity_id(db, follow_activity_id).await? else {
        return Ok(());
    };
    if follow.target_actor_uri != remote_actor.actor_uri {
        return Ok(());
    }

    let bindings = [D1Type::Text(state), D1Type::Text(follow_activity_id)];
    db.prepare(
        "UPDATE follows
         SET state = ?1,
             updated_at = CURRENT_TIMESTAMP
         WHERE follow_activity_id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn upsert_remote_follow(
    db: &D1Database,
    follower: &LocalAccount,
    actor: &RemoteActorRow,
    request: &FollowAccountRequest,
    follow_activity_id: &str,
) -> Result<()> {
    let (inbox_uri, shared_inbox_uri) = load_remote_actor_inbox_uris(db, &actor.actor_uri).await?;
    let languages_json = request
        .languages
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| {
            Error::RustError(format!("failed to serialize follow languages: {error}"))
        })?;
    let bindings = [
        D1Type::Text(follower.id.as_str()),
        D1Type::Text(actor.actor_uri.as_str()),
        match inbox_uri.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        match shared_inbox_uri.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        D1Type::Text(follow_activity_id),
        D1Type::Integer(if request.reblogs.unwrap_or(true) {
            1
        } else {
            0
        }),
        D1Type::Integer(if request.notify.unwrap_or(false) {
            1
        } else {
            0
        }),
        match languages_json.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
    ];
    db.prepare(
        "INSERT INTO follows (
            id,
            follower_account_id,
            target_account_id,
            target_actor_uri,
            target_inbox_uri,
            target_shared_inbox_uri,
            follow_activity_id,
            state,
            show_reblogs,
            notify,
            languages_json,
            created_at,
            updated_at
        ) VALUES (
            lower(hex(randomblob(16))),
            ?1,
            NULL,
            ?2,
            ?3,
            ?4,
            ?5,
            'pending',
            ?6,
            ?7,
            ?8,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(follower_account_id, target_actor_uri) DO UPDATE SET
            target_inbox_uri = excluded.target_inbox_uri,
            target_shared_inbox_uri = excluded.target_shared_inbox_uri,
            follow_activity_id = excluded.follow_activity_id,
            state = 'pending',
            show_reblogs = excluded.show_reblogs,
            notify = excluded.notify,
            languages_json = excluded.languages_json,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn load_remote_actor_inbox_uris(
    db: &D1Database,
    actor_uri: &str,
) -> Result<(Option<String>, Option<String>)> {
    let actor_uri = D1Type::Text(actor_uri);
    let row = db
        .prepare(
            "SELECT inbox_uri, shared_inbox_uri
             FROM remote_actors
             WHERE actor_uri = ?1
             LIMIT 1",
        )
        .bind_refs(&actor_uri)?
        .first::<serde_json::Value>(None)
        .await?;

    Ok((
        row.as_ref()
            .and_then(|value| value.get("inbox_uri"))
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        row.as_ref()
            .and_then(|value| value.get("shared_inbox_uri"))
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
    ))
}

pub(crate) async fn load_remote_actor_delivery_inbox(
    db: &D1Database,
    actor_uri: &str,
) -> Result<Option<String>> {
    let (inbox_uri, shared_inbox_uri) = load_remote_actor_inbox_uris(db, actor_uri).await?;
    Ok(shared_inbox_uri.or(inbox_uri))
}

pub(crate) async fn load_follow_activity_id(
    db: &D1Database,
    follower_account_id: &str,
    target_actor_uri: &str,
) -> Result<Option<String>> {
    let bindings = [
        D1Type::Text(follower_account_id),
        D1Type::Text(target_actor_uri),
    ];
    let row = db
        .prepare(
            "SELECT follow_activity_id
             FROM follows
             WHERE follower_account_id = ?1
               AND target_actor_uri = ?2
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
