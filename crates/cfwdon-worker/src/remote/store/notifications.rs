use super::lookups::find_remote_status_by_id;
use crate::notifications::{
    publish_remote_status_create_stream_notifications_soft,
    publish_remote_status_update_stream_notifications_soft,
};
use crate::push::{send_remote_status_quote_notification, send_remote_status_update_notifications};
use crate::store::remote::find_cached_remote_actor_profile_by_actor_uri;
use crate::stream_hub_publish::{
    publish_remote_status_create_stream_fanout_soft,
    publish_remote_status_update_user_stream_fanout_soft,
};
use crate::time_html::now_unix_timestamp;
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use worker::{Env, Result};

/// How old a newly ingested remote status may be and still stream as a live
/// `update` event, like Mastodon's `Status::REAL_TIME_WINDOW`. A delivery that
/// arrives late (remote retry, our own job backlog) is stored and notified but
/// not pushed onto timelines, where clients would show it as new.
const REALTIME_STREAM_WINDOW_SECS: i64 = 6 * 60 * 60;

/// Whether `published_at` is recent enough to stream. An unparsable value
/// streams rather than silently dropping a live post.
fn within_realtime_stream_window(published_at: &str, now_unix: i64) -> bool {
    OffsetDateTime::parse(published_at.trim(), &Rfc3339).map_or(true, |published| {
        now_unix - published.unix_timestamp() <= REALTIME_STREAM_WINDOW_SECS
    })
}

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
            if within_realtime_stream_window(&status.published_at, now_unix_timestamp()) {
                publish_remote_status_create_stream_fanout_soft(env, db, config, &actor, &status)
                    .await;
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    const PUBLISHED: &str = "2026-10-20T13:47:41.472Z";
    const PUBLISHED_UNIX: i64 = 1_792_504_061;

    #[test]
    fn fresh_status_streams() {
        assert!(within_realtime_stream_window(
            PUBLISHED,
            PUBLISHED_UNIX + 60
        ));
        assert!(within_realtime_stream_window(
            PUBLISHED,
            PUBLISHED_UNIX + REALTIME_STREAM_WINDOW_SECS
        ));
    }

    #[test]
    fn late_status_does_not_stream() {
        // The job backlog delivered these ~15 hours late.
        assert!(!within_realtime_stream_window(
            PUBLISHED,
            PUBLISHED_UNIX + 15 * 60 * 60
        ));
    }

    #[test]
    fn unparsable_published_at_streams() {
        assert!(within_realtime_stream_window("not a date", PUBLISHED_UNIX));
    }
}
