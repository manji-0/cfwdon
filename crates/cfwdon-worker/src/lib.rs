use crate::app_cache::install_app_cache;
use crate::background_jobs::{process_due_background_jobs, reclaim_stale_background_jobs};
use crate::collections_alpha::revalidate_stale_remote_collection_item_approvals;
use crate::d1_metrics::{bind_d1_request_route, reset_d1_request_metrics};
use crate::delivery::{
    OutboxProcessQueueMessage, consume_outbox_process_queue_batch,
    enqueue_outbox_process_queue_if_pending,
};
use crate::federation::install_remote_dns_cache;
use crate::inbox::reclaim_stale_inbox_activities;
use crate::instance::{refresh_instance_activity_cache, refresh_trending_statuses_cache};
use crate::observability::log_federation_event;
use crate::polls::process_expired_polls_for_config;
use crate::relays::purge_stale_public_remote_content;
use crate::runtime_config::load_config_from_env;
use crate::scheduled_statuses::process_due_scheduled_statuses_for_config;
use crate::store::relationship::purge_expired_mutes;
use crate::tags::refresh_trending_tags_cache;
use crate::timelines::refresh_public_timeline_cache;
use crate::tracked_d1::D1Database;
use worker::{
    Context, Env, Error, MessageBatch, Request, Response, Result, ScheduleContext, ScheduledEvent,
    console_error, event,
};

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
mod deferred;
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
mod identity;
mod inbox;
mod instance;
mod lists;
mod local_polls;
mod markers;
mod media;
mod meta_placeholder_routes;
mod notifications;
mod oauth_apps;
mod oauth_store;
mod observability;
mod policy_documents;
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
mod response_cache;
mod response_utils;
mod responses;
mod router;
mod routing;
mod runtime_config;
mod scheduled_statuses;
mod search;
mod secret_storage;
mod share;
mod statuses;
mod store;
mod stream_hub;
mod stream_hub_publish;
mod streaming_home_batch;
mod streaming_types;
mod suggestions;
mod tag_actions;
mod tags;
mod time_html;
mod timelines;
mod tracked_d1;
mod trends_cache;
mod ui_assets;
mod web_api;
mod web_ui;

#[event(fetch, respond_with_errors)]
async fn fetch(req: Request, env: Env, ctx: Context) -> Result<Response> {
    let response = router::handle_fetch(req, env).await;
    // Loops until empty, so work deferred by a deferred task also runs.
    ctx.wait_until(deferred::run_deferred_tasks_inline());
    response
}

const SCHEDULED_CRON_HOURLY: &str = "17 * * * *";
const SCHEDULED_CRON_TRENDS: &str = "17 */6 * * *";

fn scheduled_runs_hourly_maintenance(cron: &str) -> bool {
    cron == SCHEDULED_CRON_HOURLY
}

fn scheduled_runs_trend_refresh(cron: &str) -> bool {
    cron == SCHEDULED_CRON_TRENDS
}

#[event(scheduled)]
async fn scheduled(event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    let cron = event.cron();
    let run_hourly = scheduled_runs_hourly_maintenance(&cron);
    let run_trends = scheduled_runs_trend_refresh(&cron);
    if !run_hourly && !run_trends {
        console_error!("scheduled: unknown cron trigger {cron}");
        return;
    }

    reset_d1_request_metrics();
    bind_d1_request_route("scheduled");
    // Same full env/secret load as fetch and queue so cron can decrypt account keys.
    let config = load_config_from_env(&env);
    install_remote_dns_cache(&env, &config.remote_dns_cache_binding);
    install_app_cache(&env, &config.app_cache_binding);
    let result = async {
        let db = D1Database::new(env.d1(&config.database_binding)?);
        if run_hourly {
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
            if let Err(error) = purge_expired_mutes(&db).await {
                console_error!("expired mute purge failed: {error}");
            }
            if let Err(error) = purge_stale_public_remote_content(&db).await {
                console_error!("public remote retention purge failed: {error}");
            }
            if let Err(error) = refresh_instance_activity_cache(&db).await {
                console_error!("instance activity cache refresh failed: {error}");
            }
            if let Err(error) = refresh_public_timeline_cache(&db, &config).await {
                console_error!("public timeline cache refresh failed: {error}");
            }
        }
        if run_trends {
            if let Err(error) = refresh_trending_tags_cache(&db, &config).await {
                console_error!("trending tags cache refresh failed: {error}");
            }
            if let Err(error) = refresh_trending_statuses_cache(&db, &config).await {
                console_error!("trending statuses cache refresh failed: {error}");
            }
        }
        Ok::<(), Error>(())
    }
    .await;
    if let Err(error) = result {
        console_error!("scheduled maintenance failed: {error}");
    }
    deferred::run_deferred_tasks_inline().await;
}

#[cfg(test)]
mod scheduled_tests {
    use super::{
        SCHEDULED_CRON_HOURLY, SCHEDULED_CRON_TRENDS, scheduled_runs_hourly_maintenance,
        scheduled_runs_trend_refresh,
    };

    #[test]
    fn scheduled_cron_triggers_are_partitioned() {
        assert!(scheduled_runs_hourly_maintenance(SCHEDULED_CRON_HOURLY));
        assert!(!scheduled_runs_hourly_maintenance(SCHEDULED_CRON_TRENDS));
        assert!(scheduled_runs_trend_refresh(SCHEDULED_CRON_TRENDS));
        assert!(!scheduled_runs_trend_refresh(SCHEDULED_CRON_HOURLY));
    }
}

#[event(queue)]
async fn queue(
    batch: MessageBatch<OutboxProcessQueueMessage>,
    env: Env,
    _ctx: Context,
) -> Result<()> {
    let result = consume_outbox_process_queue_batch(batch, env).await;
    deferred::run_deferred_tasks_inline().await;
    result
}

#[cfg(test)]
mod compat_tests;

#[cfg(test)]
mod test_fixtures;
