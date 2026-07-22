use super::{
    AppConfig, D1Database, Error, LocalAccount, StatusRow, Visibility,
    activitypub_audiences_for_status, build_add_featured_activity, build_remove_featured_activity,
    build_status_update_activity, enqueue_targeted_outbox_activity,
    is_public_activitypub_visibility, list_follower_delivery_targets,
    load_remote_actor_delivery_inbox, local_status_target_uri, local_username_from_actor_uri,
};
use std::collections::HashSet;
use worker::Result;
use worker::d1::D1Type;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct OutboundActivityDescriptor {
    pub(crate) activity_id: String,
    pub(crate) activity_type: String,
}

pub(crate) async fn enqueue_outbound_activity(
    db: &D1Database,
    account_id: &str,
    activity_id: &str,
    activity_type: &str,
    target_actor_uri: Option<&str>,
    target_inbox: &str,
    payload_json: &str,
) -> Result<()> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(activity_id),
        D1Type::Text(activity_type),
        match target_actor_uri {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        D1Type::Text(target_inbox),
        D1Type::Text(payload_json),
    ];
    db.prepare(
        "INSERT INTO outbound_activities (
            id,
            account_id,
            activity_id,
            activity_type,
            target_actor_uri,
            target_inbox,
            payload_json,
            state,
            attempt_count,
            last_attempt_at,
            next_attempt_at,
            created_at,
            updated_at
        ) VALUES (
            lower(hex(randomblob(16))),
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            ?6,
            'queued',
            0,
            NULL,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(activity_id) DO UPDATE SET
            activity_type = excluded.activity_type,
            target_actor_uri = excluded.target_actor_uri,
            target_inbox = excluded.target_inbox,
            payload_json = excluded.payload_json,
            state = 'queued',
            attempt_count = 0,
            last_attempt_at = NULL,
            next_attempt_at = CURRENT_TIMESTAMP,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) fn describe_outbound_activity(payload_json: &str) -> Result<OutboundActivityDescriptor> {
    let payload = serde_json::from_str::<serde_json::Value>(payload_json)
        .map_err(|error| Error::RustError(format!("failed to parse outbound activity: {error}")))?;
    let activity_id = payload
        .get("id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("outbound activity is missing id".to_owned()))?
        .to_owned();
    let activity_type = payload
        .get("type")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("outbound activity is missing type".to_owned()))?
        .to_owned();

    Ok(OutboundActivityDescriptor {
        activity_id,
        activity_type,
    })
}

pub(crate) async fn queue_remote_actor_activity(
    db: &D1Database,
    account_id: &str,
    target_actor_uri: &str,
    payload_json: &str,
) -> Result<Option<String>> {
    let Some(target_inbox) = load_remote_actor_delivery_inbox(db, target_actor_uri).await? else {
        return Ok(None);
    };
    let descriptor = describe_outbound_activity(payload_json)?;
    enqueue_outbound_activity(
        db,
        account_id,
        &descriptor.activity_id,
        &descriptor.activity_type,
        Some(target_actor_uri),
        &target_inbox,
        payload_json,
    )
    .await?;
    Ok(Some(descriptor.activity_id))
}

pub(crate) async fn queue_remote_actor_activity_required(
    db: &D1Database,
    account_id: &str,
    target_actor_uri: &str,
    payload_json: &str,
) -> Result<String> {
    queue_remote_actor_activity(db, account_id, target_actor_uri, payload_json)
        .await?
        .ok_or_else(|| Error::RustError("remote account is missing a delivery inbox".to_owned()))
}

pub(crate) async fn enqueue_profile_update_activities(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
) -> Result<()> {
    let payload_json = crate::build_update_person_activity(config, account)?;
    let inboxes = list_follower_delivery_targets(db, account.id()).await?;
    enqueue_targeted_outbox_activity(db, account.id(), account.id(), &payload_json, &inboxes).await
}

pub(crate) async fn enqueue_status_update_activity(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
) -> Result<()> {
    let payload_json = build_status_update_activity(db, config, account, status).await?;
    let inboxes = match status.visibility {
        Visibility::Public | Visibility::Unlisted | Visibility::FollowersOnly => {
            list_follower_delivery_targets(db, account.id()).await?
        }
        Visibility::Direct => {
            let (to_audiences, _) =
                activitypub_audiences_for_status(db, config, account, status).await?;
            let mut inboxes = Vec::new();
            let mut seen = HashSet::new();
            let actor_uris = match &to_audiences {
                serde_json::Value::Array(values) => values
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_owned))
                    .collect::<Vec<_>>(),
                serde_json::Value::String(uri) => vec![uri.clone()],
                _ => Vec::new(),
            };
            for actor_uri in actor_uris {
                if local_username_from_actor_uri(config, &actor_uri).is_some() {
                    continue;
                }
                if let Some(inbox) = load_remote_actor_delivery_inbox(db, &actor_uri).await?
                    && seen.insert(inbox.clone())
                {
                    inboxes.push(inbox);
                }
            }
            inboxes
        }
    };
    if inboxes.is_empty() {
        return Ok(());
    }
    enqueue_targeted_outbox_activity(db, account.id(), &status.id, &payload_json, &inboxes).await
}

pub(crate) async fn enqueue_add_featured_status_activity(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
) -> Result<()> {
    if !is_public_activitypub_visibility(status.visibility.as_str()) {
        return Ok(());
    }

    let payload_json =
        build_add_featured_activity(config, account, &local_status_target_uri(status))?;
    let inboxes = list_follower_delivery_targets(db, account.id()).await?;
    enqueue_targeted_outbox_activity(db, account.id(), &status.id, &payload_json, &inboxes).await
}

pub(crate) async fn enqueue_remove_featured_status_activity(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
) -> Result<()> {
    if !is_public_activitypub_visibility(status.visibility.as_str()) {
        return Ok(());
    }

    let payload_json =
        build_remove_featured_activity(config, account, &local_status_target_uri(status))?;
    let inboxes = list_follower_delivery_targets(db, account.id()).await?;
    enqueue_targeted_outbox_activity(db, account.id(), &status.id, &payload_json, &inboxes).await
}
