use super::{
    AppConfig, LocalAccount, RemoteActorProfile, StatusRow, activity_object_id,
    delete_remote_favourite, delete_remote_reblog, extract_remote_note_object,
    fanout_and_delete_remote_status_by_object_uri_soft, fetch_remote_actor_profile,
    find_cached_remote_actor_profile_by_actor_uri, find_conversation_id_by_status_id,
    find_local_status_by_object_uri, find_remote_status_by_url_or_object_uri, is_blocking_actor,
    is_public_activitypub_visibility, is_remote_actor_following_local_account,
    list_conversation_participants, log_json_event,
    publish_remote_status_interaction_notification_soft, resolve_remote_status_by_url,
    upsert_remote_actor, upsert_remote_reblog, upsert_remote_reblog_status, upsert_remote_status,
    visibility_from_activitypub_object,
};
use worker::{Env, Result};

use crate::D1Database;
pub(crate) async fn remote_actor_may_interact_with_local_status(
    db: &D1Database,
    status: &StatusRow,
    remote_actor_uri: &str,
) -> Result<bool> {
    if is_blocking_actor(db, &status.account_id, remote_actor_uri).await? {
        return Ok(false);
    }

    match status.visibility.as_str() {
        "public" | "unlisted" => Ok(true),
        "private" => {
            is_remote_actor_following_local_account(db, &status.account_id, remote_actor_uri).await
        }
        "direct" => {
            let Some(conversation_id) = find_conversation_id_by_status_id(db, &status.id).await?
            else {
                return Ok(false);
            };
            let participants = list_conversation_participants(db, &conversation_id).await?;
            Ok(participants.iter().any(|participant| {
                participant == remote_actor_uri
                    || participant.eq_ignore_ascii_case(remote_actor_uri)
            }))
        }
        _ => Ok(false),
    }
}

pub(crate) async fn handle_inbox_like(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    account: &LocalAccount,
    config: &AppConfig,
    env: Option<&Env>,
) -> Result<()> {
    let Some(object_uri) = activity_object_id(activity.get("object")) else {
        return Ok(());
    };
    let Some(status) = find_local_status_by_object_uri(db, config, object_uri).await? else {
        return Ok(());
    };
    if status.account_id != account.id() {
        return Ok(());
    }
    if !remote_actor_may_interact_with_local_status(db, &status, &remote_actor.actor_uri).await? {
        return Ok(());
    }
    let activity_uri = activity.get("id").and_then(serde_json::Value::as_str);
    crate::upsert_remote_favourite(
        db,
        &remote_actor.actor_uri,
        &status.id,
        object_uri,
        activity_uri,
    )
    .await?;

    let _ = publish_remote_status_interaction_notification_soft(
        env,
        db,
        config,
        &status.account_id,
        remote_actor,
        "favourite",
        &status,
    )
    .await;

    Ok(())
}

pub(crate) async fn handle_inbox_announce(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    account: &LocalAccount,
    config: &AppConfig,
    env: Option<&Env>,
) -> Result<()> {
    let Some(object_uri) = activity_object_id(activity.get("object")) else {
        return Ok(());
    };

    upsert_remote_actor(db, remote_actor).await?;
    // Timeline embedding needs the boosted Note; URI-only Announces previously
    // left an empty wrapper with reblog=null.
    ensure_announce_boost_target(db, config, activity, object_uri, env).await?;
    upsert_remote_reblog_status(db, config, remote_actor, activity).await?;

    if let Some(status) = find_local_status_by_object_uri(db, config, object_uri).await? {
        if status.account_id != account.id() {
            return Ok(());
        }
        if !remote_actor_may_interact_with_local_status(db, &status, &remote_actor.actor_uri)
            .await?
        {
            return Ok(());
        }
        let activity_uri = activity.get("id").and_then(serde_json::Value::as_str);
        upsert_remote_reblog(
            db,
            &remote_actor.actor_uri,
            &status.id,
            object_uri,
            activity_uri,
        )
        .await?;

        let _ = publish_remote_status_interaction_notification_soft(
            env,
            db,
            config,
            &status.account_id,
            remote_actor,
            "reblog",
            &status,
        )
        .await;
    }

    Ok(())
}

/// Persist the Announce target Note when missing so API wrappers can embed `reblog`.
async fn ensure_announce_boost_target(
    db: &D1Database,
    config: &AppConfig,
    activity: &serde_json::Value,
    object_uri: &str,
    env: Option<&Env>,
) -> Result<()> {
    if find_local_status_by_object_uri(db, config, object_uri)
        .await?
        .is_some()
    {
        return Ok(());
    }
    if find_remote_status_by_url_or_object_uri(db, object_uri)
        .await?
        .is_some()
    {
        return Ok(());
    }

    if let Some(object) = extract_remote_note_object(activity)
        && upsert_embedded_announce_target(db, config, object, env).await?
    {
        return Ok(());
    }

    match resolve_remote_status_by_url(db, config, object_uri, None).await {
        Ok(Some(_)) => Ok(()),
        Ok(None) => {
            log_json_event(serde_json::json!({
                "event": "announce_boost_target_unresolved",
                "object_uri": object_uri,
            }));
            Ok(())
        }
        Err(error) => {
            log_json_event(serde_json::json!({
                "event": "announce_boost_target_fetch_failed",
                "object_uri": object_uri,
                "error": error.to_string(),
            }));
            Ok(())
        }
    }
}

/// Upsert an embedded Note using its `attributedTo` actor (not the Announcer).
///
/// Returns `Ok(true)` when the Note was stored.
async fn upsert_embedded_announce_target(
    db: &D1Database,
    config: &AppConfig,
    object: &serde_json::Value,
    env: Option<&Env>,
) -> Result<bool> {
    let Some((_object_id, attributed_uri)) = acceptable_embedded_announce_target(object) else {
        return Ok(false);
    };

    let profile = if let Some(cached) =
        find_cached_remote_actor_profile_by_actor_uri(db, attributed_uri).await?
    {
        cached
    } else {
        match fetch_remote_actor_profile(attributed_uri).await {
            Ok(profile) => profile,
            Err(_) => return Ok(false),
        }
    };
    upsert_remote_actor(db, &profile).await?;
    upsert_remote_status(db, config, &profile, object, env).await?;
    Ok(true)
}

/// Embedded Announce targets must be public and same-authority as `attributedTo`.
fn acceptable_embedded_announce_target(object: &serde_json::Value) -> Option<(&str, &str)> {
    let object_id = object.get("id").and_then(serde_json::Value::as_str)?;
    let attributed_uri = activity_object_id(object.get("attributedTo"))?;
    if cfwdon_domain::remote_status_object_authority_allowed(object_id, object_id, attributed_uri)
        .is_err()
    {
        return None;
    }
    if !is_public_activitypub_visibility(&visibility_from_activitypub_object(object)) {
        return None;
    }
    Some((object_id, attributed_uri))
}

#[cfg(test)]
mod tests {
    use super::acceptable_embedded_announce_target;

    #[test]
    fn embedded_announce_target_accepts_public_same_authority_note() {
        let object = serde_json::json!({
            "id": "https://remote.example/users/bob/statuses/1",
            "type": "Note",
            "attributedTo": "https://remote.example/users/bob",
            "to": ["https://www.w3.org/ns/activitystreams#Public"],
            "content": "<p>hi</p>"
        });
        let accepted = acceptable_embedded_announce_target(&object);
        assert_eq!(
            accepted,
            Some((
                "https://remote.example/users/bob/statuses/1",
                "https://remote.example/users/bob"
            ))
        );
    }

    #[test]
    fn embedded_announce_target_rejects_cross_authority_and_private() {
        let cross = serde_json::json!({
            "id": "https://remote.example/users/bob/statuses/1",
            "type": "Note",
            "attributedTo": "https://other.example/users/bob",
            "to": ["https://www.w3.org/ns/activitystreams#Public"],
        });
        assert!(acceptable_embedded_announce_target(&cross).is_none());

        let private = serde_json::json!({
            "id": "https://remote.example/users/bob/statuses/1",
            "type": "Note",
            "attributedTo": "https://remote.example/users/bob",
            "to": ["https://remote.example/users/bob/followers"],
        });
        assert!(acceptable_embedded_announce_target(&private).is_none());
    }
}

pub(crate) async fn handle_inbox_interaction_undo(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    config: &AppConfig,
    env: Option<&Env>,
) -> Result<()> {
    let Some(object) = activity.get("object") else {
        return Ok(());
    };
    let activity_type = crate::activitypub_primary_type(object).unwrap_or_default();
    let target_uri = object
        .get("object")
        .and_then(|value| activity_object_id(Some(value)))
        .unwrap_or_default();
    let activity_uri = object.get("id").and_then(serde_json::Value::as_str);

    match activity_type {
        "Like" => {
            delete_remote_favourite(db, &remote_actor.actor_uri, target_uri, activity_uri).await?
        }
        "Announce" => {
            delete_remote_reblog(db, &remote_actor.actor_uri, target_uri, activity_uri).await?;
            if let Some(announce_activity_id) = object.get("id").and_then(serde_json::Value::as_str)
            {
                fanout_and_delete_remote_status_by_object_uri_soft(
                    env,
                    db,
                    config,
                    remote_actor,
                    announce_activity_id,
                )
                .await?;
            }
        }
        _ => {}
    }

    Ok(())
}
