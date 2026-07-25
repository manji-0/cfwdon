use super::{
    D1Database, LocalAccount, RemoteActorProfile, Result, activity_object_id,
    fetch_remote_actor_profile, has_any_local_followers_for_remote_actor,
    is_local_account_following_remote_actor, object_has_activitypub_actor_type,
    parse_remote_actor_profile_document, upsert_remote_actor, validate_remote_actor_profile_urls,
};

pub(crate) async fn handle_inbox_actor_update(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    account: Option<&LocalAccount>,
) -> Result<()> {
    let Some(object) = activity.get("object").filter(|value| value.is_object()) else {
        return Ok(());
    };
    if !object_has_activitypub_actor_type(object) {
        return Ok(());
    }

    let object_actor_uri = activity_object_id(Some(object))
        .or_else(|| activity_object_id(activity.get("actor")))
        .unwrap_or_default();
    if object_actor_uri != remote_actor.actor_uri {
        return Ok(());
    }

    let is_relevant = match account {
        Some(account) => {
            is_local_account_following_remote_actor(db, account.id(), &remote_actor.actor_uri)
                .await?
        }
        None => has_any_local_followers_for_remote_actor(db, &remote_actor.actor_uri).await?,
    };
    if !is_relevant {
        return Ok(());
    }

    let refreshed = parse_remote_actor_profile_document(object, &remote_actor.actor_uri)?;
    validate_remote_actor_profile_urls(&refreshed).await?;
    let profile = if remote_actor_delivery_identity_changed(remote_actor, &refreshed) {
        // Confirm key/inbox changes against the canonical actor document.
        let confirmed = fetch_remote_actor_profile(&refreshed.actor_uri).await?;
        if confirmed.actor_uri != refreshed.actor_uri
            || confirmed.public_key_pem != refreshed.public_key_pem
            || confirmed.public_key_id != refreshed.public_key_id
            || confirmed.inbox_uri != refreshed.inbox_uri
            || confirmed.shared_inbox_uri != refreshed.shared_inbox_uri
        {
            return Err(worker::Error::RustError(
                "remote actor Update identity fields did not match canonical fetch".to_owned(),
            ));
        }
        confirmed
    } else {
        refreshed
    };
    upsert_remote_actor(db, &profile).await
}

fn remote_actor_delivery_identity_changed(
    previous: &RemoteActorProfile,
    refreshed: &RemoteActorProfile,
) -> bool {
    previous.public_key_pem != refreshed.public_key_pem
        || previous.public_key_id != refreshed.public_key_id
        || previous.inbox_uri != refreshed.inbox_uri
        || previous.shared_inbox_uri != refreshed.shared_inbox_uri
}
