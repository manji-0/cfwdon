use cfwdon_core::AppConfig;
use cfwdon_domain::{
    InstanceCapabilities, InstanceSummary, LocalAccount, ProfileField, SoftwareInfo, StatusDraft,
    Visibility,
};
use serde::Serialize;
use worker::*;

mod accounts;
mod activitypub;
mod async_refreshes;
mod auth;
mod authorize_interaction;
mod collections_alpha;
mod content_helpers;
mod conversation_store;
mod conversations;
mod crypto_keys;
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
mod push;
mod relationship;
mod relationships;
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
mod streaming_types;
mod suggestions;
mod tag_actions;
mod tags;
mod time_html;
mod timelines;
pub(crate) use accounts::*;
pub(crate) use activitypub::*;
pub(crate) use async_refreshes::*;
pub(crate) use auth::*;
pub(crate) use authorize_interaction::*;
pub(crate) use collections_alpha::*;
pub(crate) use content_helpers::*;
pub(crate) use conversation_store::*;
pub(crate) use conversations::*;
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
pub(crate) use push::*;
pub(crate) use relationship::*;
pub(crate) use relationships::*;
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
pub(crate) use streaming_types::*;
pub(crate) use tag_actions::*;
pub(crate) use tags::*;
pub(crate) use time_html::*;
pub(crate) use timelines::*;

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
    let result = async {
        let db = env.d1(&config.database_binding)?;
        match enqueue_outbox_process_queue_if_pending(&env, &db, "scheduled").await {
            Ok(true) => {}
            Ok(false) => {}
            Err(error) => console_error!("outbox delivery queue kick failed: {error}"),
        }
        if let Err(error) =
            revalidate_stale_remote_collection_item_approvals(&db, &config, 50).await
        {
            console_error!("remote collection approval revalidation failed: {error}");
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
    let db = env.d1(&config.database_binding)?;
    if !pending_outbox_work_exists(&db).await? {
        console_log!("outbox queue batch idle: messages={message_count}");
        return Ok(());
    }

    match process_outbox_deliveries_for_config(&db, &config).await {
        Ok(summary) => {
            console_log!(
                "outbox queue processed: messages={} expanded={} delivered={} failed={} completed_without_targets={}",
                message_count,
                summary.expanded,
                summary.delivered,
                summary.failed,
                summary.completed_without_targets
            );
        }
        Err(error) => {
            console_error!("outbox queue processing failed: {error}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod compat_tests;

#[cfg(test)]
mod unit_tests;
