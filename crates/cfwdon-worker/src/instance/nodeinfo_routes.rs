use super::{
    Env, build_nodeinfo_21_document, build_nodeinfo_document_with_halfyear,
    build_nodeinfo_links_document,
};
use crate::db_session::bind_request_d1;
use crate::response_cache::{cache_instance_document, cached_instance_document};
use crate::response_utils::cache_public_response;
use crate::runtime_config::{load_config, load_config_from_env};
use crate::store::instance::{
    load_active_halfyear_users, load_active_month_users, load_instance_summary,
    load_total_local_accounts, load_total_local_statuses,
};
use crate::tracked_d1::D1Database;
use worker::{Response, Result, RouteContext};

pub(crate) async fn nodeinfo_links_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    nodeinfo_links_response_for_config(&config)
}

pub(crate) fn nodeinfo_links_response_from_env(env: &Env) -> Result<Response> {
    let config = load_config_from_env(env);
    nodeinfo_links_response_for_config(&config)
}

fn nodeinfo_links_response_for_config(config: &super::AppConfig) -> Result<Response> {
    cache_public_response(
        Response::from_json(&build_nodeinfo_links_document(config))?,
        300,
    )
}

pub(crate) async fn nodeinfo_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    nodeinfo_response_for_config(&db, config).await
}

pub(crate) async fn nodeinfo_response_from_env(env: &Env) -> Result<Response> {
    let config = load_config_from_env(env);
    let db = D1Database::new(env.d1(&config.database_binding)?);
    nodeinfo_response_for_config(&db, config).await
}

/// Nodeinfo aggregates count every local status, so documents are cached.
const CACHE_TTL_NODEINFO: u32 = 300;

async fn nodeinfo_response_for_config(
    db: &D1Database,
    config: super::AppConfig,
) -> Result<Response> {
    if let Some(response) = cached_instance_document(&config, "/nodeinfo/2.0").await? {
        return cache_public_response(response, CACHE_TTL_NODEINFO);
    }
    let (summary, active_month, active_halfyear, user_count, status_count) = futures_util::try_join!(
        load_instance_summary(db, config.clone()),
        load_active_month_users(db),
        load_active_halfyear_users(db),
        load_total_local_accounts(db),
        load_total_local_statuses(db),
    )?;
    let document = build_nodeinfo_document_with_halfyear(
        &summary,
        &config,
        user_count,
        active_month,
        active_halfyear,
        status_count,
    );
    cache_instance_document(&config, "/nodeinfo/2.0", &document, CACHE_TTL_NODEINFO).await?;
    cache_public_response(Response::from_json(&document)?, CACHE_TTL_NODEINFO)
}

pub(crate) async fn nodeinfo_21_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    nodeinfo_21_response_for_config(&db, config).await
}

pub(crate) async fn nodeinfo_21_response_from_env(env: &Env) -> Result<Response> {
    let config = load_config_from_env(env);
    let db = D1Database::new(env.d1(&config.database_binding)?);
    nodeinfo_21_response_for_config(&db, config).await
}

async fn nodeinfo_21_response_for_config(
    db: &D1Database,
    config: super::AppConfig,
) -> Result<Response> {
    if let Some(response) = cached_instance_document(&config, "/nodeinfo/2.1").await? {
        return cache_public_response(response, CACHE_TTL_NODEINFO);
    }
    let (summary, active_month, active_halfyear, user_count, status_count) = futures_util::try_join!(
        load_instance_summary(db, config.clone()),
        load_active_month_users(db),
        load_active_halfyear_users(db),
        load_total_local_accounts(db),
        load_total_local_statuses(db),
    )?;
    let document = build_nodeinfo_21_document(
        &summary,
        &config,
        user_count,
        active_month,
        active_halfyear,
        status_count,
    );
    cache_instance_document(&config, "/nodeinfo/2.1", &document, CACHE_TTL_NODEINFO).await?;
    cache_public_response(Response::from_json(&document)?, CACHE_TTL_NODEINFO)
}
