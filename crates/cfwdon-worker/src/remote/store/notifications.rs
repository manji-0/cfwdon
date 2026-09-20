use super::lookups::find_remote_status_by_id;
use crate::{
    AppConfig, D1Database, find_cached_remote_actor_profile_by_actor_uri,
    publish_remote_status_create_stream_fanout_soft,
    publish_remote_status_create_stream_notifications_soft,
    publish_remote_status_update_stream_notifications_soft,
    publish_remote_status_update_user_stream_fanout_soft, send_remote_status_quote_notification,
    send_remote_status_update_notifications,
};
use worker::{Env, Result};

pub(crate) fn remote_status_notify_payload(status_id: &str, actor_uri: &str, kind: &str) -> String {
    serde_json::json!({
        "status_id": status_id,
        "actor_uri": actor_uri,
        "kind": kind,
    })
    .to_string()
}

pub(crate) async fn dispatch_remote_status_notifications(
    env: Option<&Env>,
    db: &D1Database,
    config: &AppConfig,
    status_id: &str,
    actor_uri: &str,
    kind: &str,
) -> Result<()> {
    let Some(actor) = find_cached_remote_actor_profile_by_actor_uri(db, actor_uri).await? else {
        return Ok(());
    };
    let Some(status) = find_remote_status_by_id(db, status_id).await? else {
        return Ok(());
    };

    match kind {
        "create" => {
            let _ = send_remote_status_quote_notification(
                db,
                config,
                &status.id,
                &status.actor_uri,
                status.quote_state.as_str(),
                status.quote_of_uri.as_deref(),
            )
            .await;
            publish_remote_status_create_stream_notifications_soft(
                env, db, config, &actor, &status,
            )
            .await;
            publish_remote_status_create_stream_fanout_soft(env, db, config, &actor, &status).await;
        }
        "update" => {
            let _ = send_remote_status_update_notifications(
                db,
                config,
                &status.id,
                &status.actor_uri,
                &status.object_uri,
            )
            .await;
            publish_remote_status_update_stream_notifications_soft(
                env, db, config, &actor, &status,
            )
            .await;
            if let Some(env) = env {
                publish_remote_status_update_user_stream_fanout_soft(
                    env, db, config, &actor, &status,
                )
                .await;
            }
        }
        _ => {}
    }

    Ok(())
}
