use super::upsert_follower;
use crate::activitypub::{build_accept_activity, follow_targets_local_actor};
use crate::collections_alpha::{
    handle_inbox_collection_feature_accept, handle_inbox_collection_feature_reject,
};
use crate::delivery::queue_remote_actor_activity_required;
use crate::federation::{RemoteActorProfile, remote_actor_domain_restrictions};
use crate::follow_requests::{delete_remote_follow_request_by_actor, upsert_remote_follow_request};
use crate::identity::actor_url;
use crate::notifications::publish_remote_actor_notification_soft;
use crate::relationship::update_follow_state_from_response;
use crate::time_html::now_iso_string;
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::{FollowInboxResponse, LocalAccount};
use worker::{Env, Result};
pub(crate) async fn handle_inbox_follow(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    env: Option<&Env>,
) -> Result<()> {
    if !follow_targets_local_actor(
        activity.get("object"),
        &actor_url(config, account.username()),
    ) {
        return Ok(());
    }

    // Mastodon's inbound `Follow` handling makes a silenced account's follow a
    // request, even to an unlocked account.
    let requires_approval = account.is_locked()
        || remote_actor_domain_restrictions(&remote_actor.domain, &remote_actor.actor_uri)
            .await?
            .silenced;
    let follow_activity_id = activity.get("id").and_then(serde_json::Value::as_str);

    if requires_approval {
        upsert_remote_follow_request(db, account.id(), remote_actor, follow_activity_id).await?;
        if let Ok(created_at) = now_iso_string() {
            publish_remote_actor_notification_soft(
                env,
                config,
                account.id(),
                remote_actor,
                "follow_request",
                created_at,
            )
            .await;
        }
        return Ok(());
    }

    delete_remote_follow_request_by_actor(
        db,
        account.id(),
        &remote_actor.actor_uri,
        &remote_actor.actor_uri,
    )
    .await?;
    upsert_follower(db, account.id(), remote_actor, follow_activity_id).await?;

    if let Ok(created_at) = now_iso_string() {
        publish_remote_actor_notification_soft(
            env,
            config,
            account.id(),
            remote_actor,
            "follow",
            created_at,
        )
        .await;
    }

    let accept_activity =
        build_accept_activity(config, account, activity, &remote_actor.actor_uri)?;
    queue_remote_actor_activity_required(
        db,
        account.id(),
        &remote_actor.actor_uri,
        &accept_activity,
    )
    .await?;

    Ok(())
}

pub(crate) async fn handle_inbox_accept(
    db: &D1Database,
    config: &AppConfig,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<()> {
    if handle_inbox_collection_feature_accept(db, config, activity, remote_actor).await? {
        return Ok(());
    }
    update_follow_state_from_response(
        db,
        activity,
        remote_actor,
        FollowInboxResponse::Accept.as_str(),
    )
    .await
}

pub(crate) async fn handle_inbox_reject(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<()> {
    if handle_inbox_collection_feature_reject(db, activity, remote_actor).await? {
        return Ok(());
    }
    update_follow_state_from_response(
        db,
        activity,
        remote_actor,
        FollowInboxResponse::Reject.as_str(),
    )
    .await
}
