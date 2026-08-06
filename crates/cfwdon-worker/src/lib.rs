use cfwdon_core::AppConfig;
use cfwdon_domain::{
    InstanceCapabilities, InstanceSummary, LocalAccount, ProfileField, SoftwareInfo, StatusDraft,
    Visibility,
};
use serde::Serialize;
use worker::*;

mod accounts;
mod activitypub;
mod admin_api;
mod admin_ui;
mod app_cache;
mod async_refreshes;
mod auth;
mod authorize_interaction;
mod background_jobs;
mod collections_alpha;
mod content_helpers;
mod conversation_store;
mod conversations;
mod crypto_keys;
mod custom_emojis;
mod d1_metrics;
mod db_session;
mod db_utils;
mod delivery;
mod discovery;
mod domain_blocks;
mod featured_tags;
mod federation;
mod filters;
mod follow_requests;
mod home_timeline;
mod http;
mod id_utils;
mod inbox;
mod instance;
mod lists;
mod local_polls;
mod markers;
mod media;
mod meta_placeholder_routes;
mod notifications;
mod oauth_apps;
mod observability;
mod polls;
mod profile;
mod public_endpoint_cache;
mod push;
mod relationship;
mod relationships;
mod relays;
mod remote;
mod reports;
mod request_utils;
mod response;
mod responses;
mod router;
mod routing;
mod runtime_config;
mod scheduled_statuses;
mod search;
mod secret_storage;
mod share;
mod statuses;
mod stream_hub;
mod stream_hub_publish;
mod streaming_types;
mod suggestions;
mod tag_actions;
mod tags;
mod time_html;
mod timelines;
mod tracked_d1;
pub(crate) use accounts::*;
pub(crate) use activitypub::*;
pub(crate) use admin_api::*;
pub(crate) use admin_ui::*;
pub(crate) use app_cache::*;
pub(crate) use async_refreshes::*;
pub(crate) use auth::*;
pub(crate) use authorize_interaction::*;
pub(crate) use background_jobs::*;
pub(crate) use collections_alpha::*;
pub(crate) use content_helpers::*;
pub(crate) use conversation_store::*;
pub(crate) use conversations::*;
pub(crate) use custom_emojis::*;
pub(crate) use d1_metrics::*;
pub(crate) use db_session::*;
pub(crate) use db_utils::*;
pub(crate) use delivery::*;
pub(crate) use discovery::*;
pub(crate) use domain_blocks::*;
pub(crate) use featured_tags::*;
pub(crate) use federation::*;
pub(crate) use filters::*;
pub(crate) use follow_requests::*;
pub(crate) use home_timeline::*;
pub(crate) use http::*;
pub(crate) use id_utils::*;
pub(crate) use inbox::*;
pub(crate) use instance::*;
pub(crate) use lists::*;
pub(crate) use local_polls::*;
pub(crate) use markers::*;
pub(crate) use media::*;
pub(crate) use meta_placeholder_routes::*;
pub(crate) use notifications::*;
pub(crate) use oauth_apps::*;
pub(crate) use observability::*;
pub(crate) use polls::*;
pub(crate) use profile::*;
pub(crate) use public_endpoint_cache::*;
pub(crate) use push::*;
pub(crate) use relationship::*;
pub(crate) use relationships::*;
pub(crate) use relays::*;
pub(crate) use remote::*;
pub(crate) use reports::*;
pub(crate) use request_utils::*;
pub(crate) use response::*;
pub(crate) use responses::*;
pub(crate) use routing::*;
pub(crate) use runtime_config::*;
pub(crate) use scheduled_statuses::*;
pub(crate) use search::*;
pub(crate) use share::*;
#[allow(unused_imports)]
pub(crate) use statuses::*;
#[allow(unused_imports)]
pub(crate) use stream_hub::*;
pub(crate) use stream_hub_publish::*;
pub(crate) use streaming_types::*;
pub(crate) use tag_actions::*;
pub(crate) use tags::*;
pub(crate) use time_html::*;
pub(crate) use timelines::*;
pub(crate) use tracked_d1::{D1Database, D1PreparedStatement};

#[event(fetch, respond_with_errors)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    router::handle_fetch(req, env).await
}

fn optional_env_var(env: &Env, key: &str) -> Option<String> {
    env.var(key).ok().map(|value| value.to_string())
}

fn scheduled_config(env: &Env) -> AppConfig {
    AppConfig::new(
        normalize_configured_instance_domain(
            &optional_env_var(env, "INSTANCE_DOMAIN").unwrap_or_else(|| "example.com".to_owned()),
        ),
        optional_env_var(env, "INSTANCE_NAME").unwrap_or_else(|| "cfwdon".to_owned()),
        optional_env_var(env, "INSTANCE_DESCRIPTION").unwrap_or_else(|| {
            "Cloudflare Workers + D1 + R2 based Mastodon-compatible server".to_owned()
        }),
    )
}

#[event(scheduled)]
async fn scheduled(_event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    let config = scheduled_config(&env);
    install_remote_dns_cache(&env, &config.remote_dns_cache_binding);
    install_app_cache(&env, &config.app_cache_binding);
    let result = async {
        let db = D1Database::new(env.d1(&config.database_binding)?);
        match enqueue_outbox_process_queue_if_pending(&env, &db, "scheduled").await {
            Ok(true) => log_federation_event(
                "outbox_queue_kick",
                "ok",
                "outbox queue kick enqueued from scheduled trigger",
                serde_json::json!({ "reason": "scheduled", "queued": true }),
            ),
            Ok(false) => {}
            Err(error) => log_federation_event(
                "outbox_queue_kick_failed",
                "failed",
                format!("outbox delivery queue kick failed: {error}"),
                serde_json::json!({
                    "reason": "scheduled",
                    "error": error.to_string(),
                }),
            ),
        }
        if let Err(error) =
            revalidate_stale_remote_collection_item_approvals(&db, &config, 50).await
        {
            console_error!("remote collection approval revalidation failed: {error}");
        }
        if let Err(error) = process_expired_polls_for_config(&db, &config, Some(&env)).await {
            console_error!("expired poll processing failed: {error}");
        }
        if let Err(error) =
            process_due_scheduled_statuses_for_config(&db, &config, Some(&env), 32).await
        {
            console_error!("due scheduled status processing failed: {error}");
        }
        match reclaim_stale_background_jobs(&db, 50).await {
            Ok(report) if report.requeued > 0 || report.failed > 0 => {
                log_federation_event(
                    "background_job_stale_reclaim",
                    "ok",
                    format!(
                        "reclaimed stale background jobs: requeued={} failed={}",
                        report.requeued, report.failed
                    ),
                    serde_json::json!({
                        "requeued": report.requeued,
                        "failed": report.failed,
                    }),
                );
            }
            Ok(_) => {}
            Err(error) => console_error!("background job stale reclaim failed: {error}"),
        }
        if let Err(error) = process_due_background_jobs(&db, &config, Some(&env), 16).await {
            console_error!("background job processing failed: {error}");
        }
        match reclaim_stale_inbox_activities(&db, 50).await {
            Ok(report) if report.marked_processed > 0 || report.released > 0 => {
                log_federation_event(
                    "inbox_stale_reclaim",
                    "ok",
                    format!(
                        "reclaimed stale inbox activities: marked={} released={}",
                        report.marked_processed, report.released
                    ),
                    serde_json::json!({
                        "marked_processed": report.marked_processed,
                        "released": report.released,
                    }),
                );
            }
            Ok(_) => {}
            Err(error) => console_error!("inbox stale reclaim failed: {error}"),
        }
        if let Err(error) = purge_stale_public_remote_content(&db).await {
            console_error!("public remote retention purge failed: {error}");
        }
        if let Err(error) = refresh_trending_tags_cache(&db, &config).await {
            console_error!("trending tags cache refresh failed: {error}");
        }
        if let Err(error) = refresh_instance_activity_cache(&db).await {
            console_error!("instance activity cache refresh failed: {error}");
        }
        if let Err(error) = refresh_trending_statuses_cache(&db, &config).await {
            console_error!("trending statuses cache refresh failed: {error}");
        }
        if let Err(error) = refresh_public_timeline_cache(&db, &config).await {
            console_error!("public timeline cache refresh failed: {error}");
        }
        Ok::<(), Error>(())
    }
    .await;
    if let Err(error) = result {
        console_error!("scheduled maintenance failed: {error}");
    }
}

#[event(queue)]
async fn queue(
    batch: MessageBatch<OutboxProcessQueueMessage>,
    env: Env,
    _ctx: Context,
) -> Result<()> {
    let message_count = batch.raw_iter().count();
    batch.ack_all();
    let config = load_config_from_env(&env);
    install_remote_dns_cache(&env, &config.remote_dns_cache_binding);
    install_app_cache(&env, &config.app_cache_binding);
    let db = D1Database::new(env.d1(&config.database_binding)?);
    if !pending_outbox_work_exists(&db).await? {
        log_federation_event(
            "outbox_queue_idle",
            "ok",
            format!("outbox queue batch idle: messages={message_count}"),
            serde_json::json!({ "messages": message_count }),
        );
        return Ok(());
    }

    match process_outbox_deliveries_for_config(&db, &config).await {
        Ok(summary) => {
            log_federation_event(
                "outbox_queue_processed",
                "ok",
                format!(
                    "outbox queue processed: messages={} expanded={} delivered={} failed={} completed_without_targets={}",
                    message_count,
                    summary.expanded,
                    summary.delivered,
                    summary.failed,
                    summary.completed_without_targets
                ),
                serde_json::json!({
                    "messages": message_count,
                    "expanded": summary.expanded,
                    "delivered": summary.delivered,
                    "failed": summary.failed,
                    "completed_without_targets": summary.completed_without_targets,
                }),
            );
            // Each run claims a bounded batch. Continue only while the previous
            // batch progressed, so a stalled backlog cannot self-schedule forever.
            if outbox_batch_made_progress(&summary) {
                match enqueue_outbox_process_queue_if_pending(&env, &db, "queue_continuation").await
                {
                    Ok(true) => log_federation_event(
                        "outbox_queue_kick",
                        "ok",
                        "outbox queue continuation enqueued",
                        serde_json::json!({
                            "reason": "queue_continuation",
                            "queued": true,
                        }),
                    ),
                    Ok(false) => {}
                    Err(error) => log_federation_event(
                        "outbox_queue_kick_failed",
                        "failed",
                        format!("outbox queue continuation failed: {error}"),
                        serde_json::json!({
                            "reason": "queue_continuation",
                            "error": error.to_string(),
                        }),
                    ),
                }
            }
        }
        Err(error) => {
            log_federation_event(
                "outbox_queue_processing_failed",
                "failed",
                format!("outbox queue processing failed: {error}"),
                serde_json::json!({
                    "messages": message_count,
                    "error": error.to_string(),
                }),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod compat_tests;

#[cfg(test)]
mod unit_tests;
