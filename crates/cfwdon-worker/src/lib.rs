use cfwdon_core::AppConfig;
use cfwdon_domain::{
    InstanceCapabilities, InstanceSummary, LocalAccount, ProfileField, SoftwareInfo, StatusDraft,
    Visibility,
};
use serde::Serialize;
use worker::*;

mod accounts;
mod activitypub;
mod activitypub_actor_document;
mod activitypub_local_uri;
mod activitypub_objects;
mod activitypub_parse;
mod activitypub_social_activities;
mod activitypub_updates;
mod async_refreshes;
mod auth;
mod auth_account_store;
mod auth_jwt;
mod authorize_interaction;
mod collections_alpha;
mod content_helpers;
mod conversation_store;
mod conversations;
mod crypto_keys;
mod db_utils;
mod delivery;
mod delivery_outbound;
mod delivery_outbound_state;
mod delivery_outbox_enqueue;
mod delivery_store;
mod delivery_store_state;
mod discovery;
mod domain_blocks;
mod featured_tags;
mod federation_fetch;
mod federation_url_guard;
mod filters;
mod follow_requests;
mod home_timeline_query;
mod home_timeline_store;
mod http;
mod id_utils;
mod inbox;
mod inbox_activity_store;
mod inbox_actor_updates;
mod inbox_follow_handlers;
mod inbox_interactions;
mod inbox_poll_interactions;
mod inbox_status_handlers;
mod inbox_status_interactions;
mod inbox_target_resolution;
mod instance;
mod instance_documents;
mod instance_identity;
mod instance_nodeinfo_documents;
mod instance_policy_documents;
mod instance_store;
mod lists;
mod local_polls;
mod markers;
mod media;
mod meta_placeholder_routes;
mod notification_account_store;
mod notification_accounts;
mod notification_admin;
mod notification_collection;
mod notification_engagement_store;
mod notification_filters;
mod notification_mention_entries;
mod notification_mention_store;
mod notification_policy;
mod notification_poll_entries;
mod notification_quote_entries;
mod notification_quote_store;
mod notification_reblog_entries;
mod notification_routes;
mod notification_state;
mod notification_status_entries;
mod notification_status_store;
mod notification_types;
mod notification_update_entries;
mod notification_update_store;
mod notifications;
mod oauth_apps;
mod observability;
mod poll_expiration_store;
mod poll_request_parsing;
mod polls;
mod profile;
mod profile_request_parsing;
mod push_delivery;
mod push_subscriptions;
mod relationship;
mod relationships;
mod remote_actor_profile_store;
mod remote_actor_store;
mod remote_poll_activity;
mod remote_poll_mutations;
mod remote_poll_parsing;
mod remote_poll_store;
mod remote_polls;
mod remote_resolve;
mod remote_status_edits;
mod remote_store;
mod report_store;
mod reports;
mod reports_request_parsing;
mod request_utils;
mod response;
mod responses;
mod router;
mod routing;
mod runtime_config;
mod scheduled_statuses;
mod search;
mod status_action_resolution;
mod status_bookmark_store;
mod status_bookmarks;
mod status_counts;
mod status_detail_routes;
mod status_edits;
mod status_favourite_store;
mod status_favourites;
mod status_local_context;
mod status_local_timeline_store;
mod status_mutations;
mod status_outbox_activities;
mod status_pins;
mod status_placeholder_routes;
mod status_reblog_store;
mod status_reblogs;
mod status_remote_context;
mod status_remote_mutations;
mod status_request_parsing;
mod status_response_builders;
mod status_store;
mod status_store_local;
mod status_store_remote;
mod status_thread_mutes;
mod statuses;
mod streaming_types;
mod suggestions;
mod tag_actions;
mod tags;
mod time_html;
mod timeline_request_parsing;
mod timeline_search;
mod timelines;
pub(crate) use accounts::*;
pub(crate) use activitypub::*;
pub(crate) use activitypub_actor_document::*;
pub(crate) use activitypub_local_uri::*;
pub(crate) use activitypub_objects::*;
pub(crate) use activitypub_parse::*;
pub(crate) use activitypub_social_activities::*;
pub(crate) use activitypub_updates::*;
pub(crate) use async_refreshes::*;
pub(crate) use auth::*;
pub(crate) use auth_account_store::*;
pub(crate) use authorize_interaction::*;
pub(crate) use collections_alpha::*;
pub(crate) use content_helpers::*;
pub(crate) use conversation_store::*;
pub(crate) use conversations::*;
pub(crate) use db_utils::*;
pub(crate) use delivery::*;
pub(crate) use delivery_outbound::*;
pub(crate) use delivery_outbound_state::*;
pub(crate) use delivery_outbox_enqueue::*;
pub(crate) use delivery_store::*;
pub(crate) use delivery_store_state::*;
pub(crate) use discovery::*;
pub(crate) use domain_blocks::*;
pub(crate) use featured_tags::*;
pub(crate) use federation_fetch::*;
pub(crate) use filters::*;
pub(crate) use follow_requests::*;
pub(crate) use home_timeline_store::*;
pub(crate) use http::*;
pub(crate) use id_utils::*;
pub(crate) use inbox::*;
pub(crate) use inbox_activity_store::*;
pub(crate) use inbox_actor_updates::*;
pub(crate) use inbox_follow_handlers::*;
pub(crate) use inbox_interactions::*;
pub(crate) use inbox_poll_interactions::*;
pub(crate) use inbox_status_handlers::*;
pub(crate) use inbox_status_interactions::*;
pub(crate) use inbox_target_resolution::*;
pub(crate) use instance::*;
pub(crate) use instance_documents::*;
pub(crate) use instance_identity::*;
pub(crate) use instance_nodeinfo_documents::*;
pub(crate) use instance_policy_documents::*;
pub(crate) use instance_store::*;
pub(crate) use lists::*;
pub(crate) use local_polls::*;
pub(crate) use markers::*;
pub(crate) use media::*;
pub(crate) use meta_placeholder_routes::*;
pub(crate) use notification_account_store::*;
pub(crate) use notification_accounts::*;
pub(crate) use notification_admin::*;
pub(crate) use notification_collection::*;
pub(crate) use notification_engagement_store::*;
pub(crate) use notification_filters::*;
pub(crate) use notification_mention_entries::*;
pub(crate) use notification_mention_store::*;
pub(crate) use notification_policy::*;
pub(crate) use notification_poll_entries::*;
pub(crate) use notification_quote_entries::*;
pub(crate) use notification_quote_store::*;
pub(crate) use notification_reblog_entries::*;
pub(crate) use notification_routes::*;
pub(crate) use notification_state::*;
pub(crate) use notification_status_entries::*;
pub(crate) use notification_status_store::*;
pub(crate) use notification_update_entries::*;
pub(crate) use notification_update_store::*;
pub(crate) use notifications::*;
pub(crate) use oauth_apps::*;
pub(crate) use observability::*;
pub(crate) use poll_expiration_store::*;
pub(crate) use poll_request_parsing::*;
pub(crate) use polls::*;
pub(crate) use profile::*;
pub(crate) use profile_request_parsing::*;
pub(crate) use push_delivery::*;
pub(crate) use push_subscriptions::*;
pub(crate) use relationship::*;
pub(crate) use relationships::*;
pub(crate) use remote_actor_profile_store::*;
pub(crate) use remote_actor_store::*;
pub(crate) use remote_poll_activity::*;
pub(crate) use remote_poll_mutations::*;
pub(crate) use remote_poll_parsing::*;
pub(crate) use remote_poll_store::*;
pub(crate) use remote_polls::*;
pub(crate) use remote_resolve::*;
pub(crate) use remote_status_edits::*;
pub(crate) use remote_store::*;
pub(crate) use report_store::*;
pub(crate) use reports::*;
pub(crate) use reports_request_parsing::*;
pub(crate) use request_utils::*;
pub(crate) use response::*;
pub(crate) use responses::*;
pub(crate) use routing::*;
pub(crate) use runtime_config::*;
pub(crate) use scheduled_statuses::*;
pub(crate) use search::*;
pub(crate) use status_action_resolution::*;
pub(crate) use status_bookmark_store::*;
pub(crate) use status_bookmarks::*;
pub(crate) use status_counts::*;
pub(crate) use status_detail_routes::*;
pub(crate) use status_edits::*;
pub(crate) use status_favourite_store::*;
pub(crate) use status_favourites::*;
pub(crate) use status_local_context::*;
pub(crate) use status_local_timeline_store::*;
pub(crate) use status_mutations::*;
pub(crate) use status_outbox_activities::*;
pub(crate) use status_pins::*;
pub(crate) use status_placeholder_routes::*;
pub(crate) use status_reblog_store::*;
pub(crate) use status_reblogs::*;
pub(crate) use status_remote_context::*;
pub(crate) use status_remote_mutations::*;
pub(crate) use status_request_parsing::*;
#[allow(unused_imports)]
pub(crate) use status_response_builders::*;
pub(crate) use status_store::*;
pub(crate) use status_store_local::*;
pub(crate) use status_store_remote::*;
pub(crate) use status_thread_mutes::*;
pub(crate) use statuses::*;
pub(crate) use streaming_types::*;
pub(crate) use suggestions::*;
pub(crate) use tag_actions::*;
pub(crate) use tags::*;
pub(crate) use time_html::*;
pub(crate) use timeline_request_parsing::*;
pub(crate) use timeline_search::*;
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
        optional_env_var(env, "INSTANCE_DOMAIN").unwrap_or_else(|| "example.com".to_owned()),
        optional_env_var(env, "INSTANCE_NAME").unwrap_or_else(|| "cfwdon".to_owned()),
        optional_env_var(env, "INSTANCE_DESCRIPTION").unwrap_or_else(|| {
            "Cloudflare Workers + D1 + R2 based Mastodon-compatible server".to_owned()
        }),
    )
}

#[event(scheduled)]
async fn scheduled(_event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    let config = scheduled_config(&env);
    let result = async {
        let db = env.d1(&config.database_binding)?;
        revalidate_stale_remote_collection_item_approvals(&db, &config, 50).await
    }
    .await;
    if let Err(error) = result {
        console_error!("remote collection approval revalidation failed: {error}");
    }
}

#[cfg(test)]
mod compat_tests;

#[cfg(test)]
mod unit_tests;
